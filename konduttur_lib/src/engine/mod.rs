use anyhow::Result;
use arc_swap::ArcSwap;
use assert_no_alloc::assert_no_alloc;
use cpal::{SampleFormat, StreamConfig};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use thiserror::Error;

pub mod assetserver;
pub mod engineconfig;
pub mod tick;

use crate::engine::{engineconfig::EngineConfig, tick::Tick};
use crate::model::{
    DataKind, Renderable,
    arr::{clip::ClipID, track::TrackID},
    asset::{Asset, AssetID},
    flow::{NativeNodeType, NodeID, NodePayload, SocketIndex},
    project::ProjectData,
};

// ---------------------------------------------------------------------
// Compiled schedule
// ---------------------------------------------------------------------
pub type SlotIndex = usize;

#[derive(Debug, Clone)]
pub struct SummingCommand {
    /// The target scratch slot where the summed audio will be collected
    pub target_scratch_slot: SlotIndex,
    /// All the source slots that need to be blended together into the scratch slot
    pub source_slots: Vec<SlotIndex>,
}

pub struct ScheduleStep {
    pub node_id: NodeID,
    /// Pre-compiled instructions telling the engine what to sum before running the node
    pub prep_sums: Vec<SummingCommand>,
    /// One entry per input socket; each entry lists every buffer slot
    /// feeding it (0 = unconnected, 1 = normal, 2+ = fan-in summed).
    pub input_slots: Vec<SlotIndex>,
    /// One entry per output socket.
    pub output_slots: Vec<SlotIndex>,
}

pub struct CompiledGraph {
    pub steps: Vec<ScheduleStep>,
    pub buffer_count: usize,
    pub master_output_slot: SlotIndex,
}

#[derive(Debug, Error)]
pub enum EngineError {
    TrackNotFound(TrackID),
    NodeNotFound(NodeID),
    IncompatibleSockets { from: DataKind, to: DataKind },
    WouldCreateCycle,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ---------------------------------------------------------------------
// Execution — runs once per audio callback
// ---------------------------------------------------------------------

pub struct BlockBufferPool {
    /// Contiguous pre-allocated block: (buffer_count * block_size)
    memory: Vec<f32>,
    block_size: usize,
}

impl BlockBufferPool {
    pub fn new(buffer_count: usize, block_size: usize) -> Self {
        Self {
            memory: vec![0.0f32; buffer_count * block_size],
            block_size,
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.memory.fill(0.0f32);
    }

    /// Creates an execution context that allows unsafe, arbitrary slot slicing
    /// while keeping the unsafe code safely isolated.
    #[inline]
    pub fn executor(&mut self) -> PoolExecutor {
        PoolExecutor {
            ptr: self.memory.as_mut_ptr(),
            block_size: self.block_size,
            // Track length to avoid out-of-bounds memory access in debug mode
            total_len: self.memory.len(),
        }
    }
}

/// A short-lived handle for zero-allocation, arbitrary buffer slicing
pub struct PoolExecutor {
    ptr: *mut f32,
    block_size: usize,
    total_len: usize,
}

impl PoolExecutor {
    /// Get a read-only view of a slot.
    /// Safety: Assumes no other code is actively writing to this slot right now.
    #[inline]
    pub fn get_input<'a>(&self, slot: SlotIndex) -> &'a [f32] {
        unsafe {
            let offset = slot * self.block_size;
            debug_assert!(offset + self.block_size <= self.total_len);
            std::slice::from_raw_parts(self.ptr.add(offset), self.block_size)
        }
    }

    /// Get a mutable view of a slot.
    /// Safety: Assumes no other code is actively reading or writing to this slot right now.
    #[inline]
    pub fn get_output<'a>(&mut self, slot: SlotIndex) -> &'a mut [f32] {
        unsafe {
            let offset = slot * self.block_size;
            debug_assert!(offset + self.block_size <= self.total_len);
            std::slice::from_raw_parts_mut(self.ptr.add(offset), self.block_size)
        }
    }
}
/// Runs the compiled schedule for one block and returns the master mix.
pub fn execute_block<'a>(
    schedule: &CompiledGraph,
    project: &ProjectData,
    block_start: Tick,
    frame_count: usize,
    channels: u16,
    pool: &'a mut BlockBufferPool,
) -> &'a [f32] {
    // assert_no_alloc(|| {
    let sample_count = frame_count * channels as usize;
    // pool.ensure(schedule.buffer_count, sample_count);

    // Wipe layout elements: Completely clean block with zero allocation overhead
    pool.clear();
    let mut executor = pool.executor();

    for i in 0..schedule.steps.len() {
        let step = &schedule.steps[i];
        let node = &project.graph.nodes[step.node_id];

        // 1. Process engine-level fan-in matrix configurations
        for sum_cmd in &step.prep_sums {
            unsafe {
                let target = executor.get_output(sum_cmd.target_scratch_slot);

                for &source_slot in &sum_cmd.source_slots {
                    let source = executor.get_input(source_slot);

                    // Simple additive loop: easily optimized by hardware auto-vectorization
                    for sample_idx in 0..pool.block_size {
                        target[sample_idx] += source[sample_idx];
                    }
                }
            }
        }

        // 2. Call the node's internal process method with pure 1-to-1 socket bindings
       ;
        let node = &project.graph.nodes[step.node_id];
        process_node(
            &node.payload,
            project,
            block_start,
            channels,
            &step.input_slots,
            &step.output_slots,
            &mut executor,
        );
    }

    // 3. Extract output safely
    executor.get_input(schedule.master_output_slot)

    // })
}

fn process_node(
    payload: &NodePayload,
    project: &ProjectData,
    block_start: Tick,
    channels: u16,
    inputs: &[SlotIndex],
    outputs: &[SlotIndex],
    pool: &mut PoolExecutor,
) {
    match payload {
        NodePayload::TrackReader(track_id) => {
            if let Some(track) = project.tracks.get(*track_id) {
                let output_buf = pool.get_output(outputs[0]);
                track.render(project, output_buf, block_start, channels);
            }
        }
        NodePayload::Native(NativeNodeType::Master) => {
            let input_buf = pool.get_input(inputs[0]);
            let output_buf = pool.get_output(outputs[0]);

            output_buf.copy_from_slice(&input_buf);
        }
        NodePayload::Native(_other) => unimplemented!("native node type not wired up yet"),
        NodePayload::Group(_) => unimplemented!("group inlining not implemented yet"),
    }
}

pub enum Command {
    AddTrack {
        name: String,
        kind: DataKind,
    },
    RemoveTrack(TrackID),
    AddClip {
        track: TrackID,
        start: Tick,
        end: Tick,
        asset: AssetID,
    },
    MoveClip {
        track: TrackID,
        clip: ClipID,
        new_start: Tick,
    },
    AddLink {
        from: (NodeID, SocketIndex),
        to: (NodeID, SocketIndex),
    },
    RemoveLink {
        from: (NodeID, SocketIndex),
        to: (NodeID, SocketIndex),
    },
    Play,
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::AddTrack { name, kind } => write!(f, "Added {kind:?} track \"{name}\""),
            Command::RemoveTrack(track_id) => write!(f, "Removed track \"{track_id:?}\""),
            Command::AddClip {
                track,
                start,
                end,
                asset,
            } => {
                write!(
                    f,
                    "Added clip {asset:?} to {track:?} at {start:?} until {end:?} ticks"
                )
            }
            Command::MoveClip {
                track,
                clip,
                new_start,
            } => todo!(),
            Command::AddLink { from, to } => todo!(),
            Command::RemoveLink { from, to } => todo!(),
            Self::Play => write!(f, "Playing track from"),
        }
    }
}

/// What the audio thread reads. Today this is "the whole Project plus its
/// compiled schedule"; splitting Project into a separately-published
/// TimelineSnapshot (per the earlier design) is a later optimization that
/// doesn't change anything on the Engine/Command side.
pub struct RenderState {
    pub project: Arc<ProjectData>,
    pub schedule: Arc<CompiledGraph>,
}

pub struct Engine {
    pub config: EngineConfig,
    pub playhead: Arc<AtomicU64>,
    current: Arc<ProjectData>,
    undo_stack: Vec<Arc<ProjectData>>,
    redo_stack: Vec<Arc<ProjectData>>,
    publish: Arc<ArcSwap<RenderState>>,
}

impl Engine {
    pub fn new(project: Arc<ProjectData>) -> Result<Self> {
        let schedule = project
            .compile_graph()
            .expect("fresh project graph is always acyclic");

        let publish = Arc::new(ArcSwap::from_pointee(RenderState {
            project: project.clone(),
            schedule: Arc::new(schedule),
        }));
        Ok(Self {
            config: EngineConfig::create()?,
            current: project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            publish,
            playhead: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Clone this and hand it to the audio callback; it's the read side of
    /// the lock-free publish.
    pub fn render_state_handle(&self) -> Arc<ArcSwap<RenderState>> {
        self.publish.clone()
    }

    pub fn project(&self) -> &ProjectData {
        &self.current
    }

    /// Not a Command on purpose — asset import is I/O-bound and, unlike
    /// graph/clip edits, isn't meaningfully undo-able in the same sense.
    /// A real engine would still route this through some queue so it
    /// doesn't block the caller, but it's direct here for clarity.
    pub fn load_asset(&mut self, asset: Asset) -> AssetID {
        let mut next = (*self.current).clone();
        let id = next.assets.insert(asset);
        self.commit(next);
        id
    }

    pub fn apply(&mut self, cmd: Command) -> Result<()> {
        let mut next = (*self.current).clone();
        println!("{cmd}");
        self.apply_command(&mut next, cmd)?;
        self.commit(next);
        Ok(())
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack
                .push(std::mem::replace(&mut self.current, prev));
            self.publish_current();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack
                .push(std::mem::replace(&mut self.current, next));
            self.publish_current();
        }
    }

    fn commit(&mut self, next: ProjectData) {
        self.undo_stack
            .push(std::mem::replace(&mut self.current, Arc::new(next)));
        self.redo_stack.clear();
        self.publish_current();
    }

    fn publish_current(&mut self) {
        let schedule = self
            .current
            .compile_graph()
            .expect("command validation should prevent cycles before this point");
        self.publish.store(Arc::new(RenderState {
            project: self.current.clone(),
            schedule: Arc::new(schedule),
        }));
    }

    fn apply_command(&self, project: &mut ProjectData, cmd: Command) -> Result<()> {
        match cmd {
            Command::AddTrack { name, kind } => project.add_track(name, kind),

            Command::RemoveTrack(track_id) => project.remove_track(track_id),
            Command::AddClip {
                track,
                start,
                end: length,
                asset,
            } => project.add_clip_to_track(track, start, length, asset),
            Command::MoveClip {
                track,
                clip,
                new_start,
            } => project.move_clip(track, clip, new_start),
            Command::AddLink { from, to } => project.add_link(from, to),
            Command::RemoveLink { from, to } => project.remove_link(from, to),
            Command::Play => self.play(),
        }
    }

    fn play(&self) -> Result<()> {
        use cpal::traits::StreamTrait;
        // --- 3. Hand the audio thread its read handle -----------------------
        let render_state: Arc<ArcSwap<RenderState>> = self.render_state_handle();
        // let playhead = Arc::new(AtomicU64::new(0));

        let stream = match self.config.sample_format {
            SampleFormat::F32 => self.build_stream::<f32>(render_state)?,
            other => anyhow::bail!(
                "device wants sample format {other:?}; only f32 output is wired up in this skeleton \
             (TODO: convert via cpal::Sample for I16/U16 devices)"
            ),
        };
        dbg!();
        println!("Press enter to play");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        // Stream has to stay alive for audio to keep playing -- this local
        // binding, held until the end of main(), is what does that.
        stream.play()?;

        println!("Playing... press enter to quit");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(())
    }

    /// Generic over the sample type cpal actually wants; only instantiated
    /// for f32 today (see the SampleFormat match above), but written this way
    /// so adding I16/U16 conversion later is a second match arm, not a rewrite.
    fn build_stream<T>(&self, render_state: Arc<ArcSwap<RenderState>>) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        use cpal::traits::DeviceTrait;

        let device_clone = self.config.device.clone();
        let config = self.config.config;
        let playhead = self.playhead.clone();
        let state = render_state.load();
        let mut pool = BlockBufferPool::new(state.schedule.buffer_count, 1024);
        let stream = device_clone.build_output_stream(
            config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                let frame_count = data.len() / config.channels as usize;
                let start =
                    playhead.fetch_add(frame_count as u64, std::sync::atomic::Ordering::Relaxed);

                // The entire real-time path: load the current published state
                // (lock-free), run the compiled schedule, copy the result out
                // converting f32 -> whatever cpal wants.
                let mixed = execute_block(
                    &state.schedule,
                    &state.project,
                    tick::Tick(start),
                    frame_count,
                    config.channels,
                    &mut pool,
                );

                for (dst, &src) in data.iter_mut().zip(mixed) {
                    *dst = T::from_sample(src);
                }
            },
            move |err| eprintln!("audio stream error: {err}"),
            None,
        )?;

        Ok(stream)
    }
}
