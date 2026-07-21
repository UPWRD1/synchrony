pub mod assetserver;
pub mod bbp;
pub mod command;
pub mod engineconfig;
pub mod tick;

use std::any::Any;
use std::marker::PhantomData;
use std::sync::{Arc, atomic::AtomicU64};

use crate::engine::bbp::BlockBufferPool;
pub use crate::engine::command::*;
use crate::engine::{engineconfig::EngineConfig, tick::Tick};
use crate::model::{
    DataKind,
    asset::{AudioAsset, AudioAssetID},
    flow::NodeID,
    project::ProjectData,
};

use anyhow::Result;
use arc_swap::ArcSwap;
use cpal::SampleFormat;
use slotmap::SecondaryMap;
use thiserror::Error;

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

pub struct GraphUpdate {
    pub project: Arc<ProjectData>,
    pub schedule: Arc<CompiledGraph>,
    pub state_additions: Vec<(NodeID, Box<dyn Any + Send>)>, // pre-built off-thread
    pub state_removals: Vec<NodeID>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    TrackNotFound,
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

struct AudioResources {
    current: Option<GraphUpdate>, // holds the live Arcs
    state_pool: SecondaryMap<NodeID, Box<dyn Any + Send>>, // with_capacity(MAX_NODES)
    buffer_pool: BlockBufferPool, // sized once, generously
    updates_rx: rtrb::Consumer<GraphUpdate>,
    trash_tx: rtrb::Producer<Box<dyn Any + Send>>,
}

/// Runs the compiled schedule for one block and returns the master mix.
pub fn execute_block<'a>(
    schedule: &CompiledGraph,
    project: &ProjectData,
    block_start: Tick,
    channels: u16,
    pool: &'a mut BlockBufferPool,
) -> &'a [f32] {
    // assert_no_alloc(|| {

    // Clear the pool. Unless you want to summon demons.
    pool.clear();

    let mut executor = pool.executor();

    for i in 0..schedule.steps.len() {
        let step = &schedule.steps[i];
        let node = &project.graph.nodes[step.node_id];

        // 1. Process engine-level fan-in matrix configurations
        for sum_cmd in &step.prep_sums {
            let target = executor.get_output(sum_cmd.target_scratch_slot);

            for &source_slot in &sum_cmd.source_slots {
                let source = executor.get_input(source_slot);

                // Simple additive loop: easily optimized by hardware auto-vectorization
                for sample_idx in 0..pool.block_size {
                    target[sample_idx] += source[sample_idx];
                }
            }
        }
        node.process(
            project,
            &mut executor,
            block_start,
            channels,
            &step.input_slots,
            &step.output_slots,
        );
    }

    executor.get_input(schedule.master_output_slot)
    // })
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
    stream: cpal::Stream,
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
        let playhead = Arc::new(AtomicU64::new(0));
        let config = EngineConfig::create()?;
        let stream = match config.sample_format {
            SampleFormat::F32 => Self::build_stream::<f32>(&config, playhead, publish.clone())?,
            other => anyhow::bail!(
                "device wants sample format {other:?}; only f32 output is wired up in this skeleton \
             (TODO: convert via cpal::Sample for I16/U16 devices)"
            ),
        };
        Ok(Self {
            config,
            current: project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            publish,
            playhead: Arc::new(AtomicU64::new(0)),
            stream,
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
    pub fn load_asset(&mut self, asset: AudioAsset) -> AudioAssetID {
        let mut next = (*self.current).clone();
        let id = next.assets.insert(asset);
        self.commit(next);
        id
    }

    pub fn apply<T>(&mut self, cmd: T) -> Result<T::Output>
    where
        T: Command,
    {
        let mut next = (*self.current).clone();

        let res = self.apply_command(&mut next, cmd)?;
        self.commit(next);
        Ok(res)
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

    fn apply_command<T>(&self, project: &mut ProjectData, cmd: T) -> Result<T::Output>
    where
        T: Command,
    {
        cmd.execute(project)
    }

    pub fn move_playhead(&self, to: Tick) -> Result<()> {
        self.playhead
            .swap(to.0, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    pub fn play(&self) -> Result<()> {
        use cpal::traits::StreamTrait;
        // --- 3. Hand the audio thread its read handle -----------------------
        let render_state: Arc<ArcSwap<RenderState>> = self.render_state_handle();
        // let playhead = Arc::new(AtomicU64::new(0));

        todo!();
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
    fn build_stream<T>(
        config: &EngineConfig,
        playhead: Arc<AtomicU64>,
        render_state: Arc<ArcSwap<RenderState>>,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        use cpal::traits::DeviceTrait;

        let device_clone = config.device.clone();
        let config = config.config;
        let playhead = playhead.clone();
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
