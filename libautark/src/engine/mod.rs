//! The core audio engine. Used to manipulate `Project`s, hold the audio thread, and more.
pub mod assetserver;
pub mod bbp;
pub mod command;
pub mod constants;
pub mod engineconfig;
pub mod errors;
pub mod state;
pub mod tick;
pub mod transport;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, atomic::AtomicU64};

use crate::engine::bbp::BlockBufferPool;
pub use crate::engine::command::*;
use crate::engine::constants::{
    GARBAGE_RING_CAPACITY, MAX_BUFFER_SLOTS, MAX_NODES, UPDATE_RING_CAPACITY,
};
use crate::engine::state::{Garbage, GraphUpdate, NodeStatePool};
use crate::engine::transport::Transport;
use crate::engine::{engineconfig::EngineConfig, tick::Tick};

use crate::model::{asset::AudioAssetID, flow::NodeID, project::ProjectData};

use anyhow::Result;

pub type SlotIndex = usize;

pub struct ScheduleStep {
    pub node_id: NodeID,
    pub input_slots: Vec<SlotIndex>,
    pub output_slots: Vec<SlotIndex>,
}

pub struct CompiledGraph {
    pub steps: Vec<ScheduleStep>,
    pub buffer_count: usize,
    pub master_output_slot: SlotIndex,
}

/// Runs the compiled schedule for one block and returns the master mix.
pub fn execute_block<'a>(
    schedule: &CompiledGraph,
    project: &ProjectData,
    block_start: Tick,
    pool: &'a mut BlockBufferPool,
    state_pool: &mut NodeStatePool,
) -> &'a [f32] {
    // assert_no_alloc(|| {

    // Clear the pool. Unless you want to summon demons.
    pool.clear();

    let mut executor = pool.executor();

    for i in 0..schedule.steps.len() {
        let step = &schedule.steps[i];
        let node = &project.graph.nodes[step.node_id];

        node.process_erased(
            &mut executor,
            state_pool.get_mut(step.node_id),
            project,
            block_start,
            &step.input_slots,
            &step.output_slots,
        );
    }

    executor.get_input(schedule.master_output_slot)
    // })
}

/// What the audio thread reads.
pub struct RenderState {
    pub project: Arc<ProjectData>,
    pub schedule: Arc<CompiledGraph>,
}

pub struct Engine {
    pub transport: Arc<Transport>,
    pub playhead: Arc<AtomicU64>,
    config: EngineConfig,
    current: Arc<ProjectData>,
    undo_stack: Vec<Arc<ProjectData>>,
    redo_stack: Vec<Arc<ProjectData>>,
    audio_manager: AudioManager,
}

pub struct AudioManager {
    update_tx: rtrb::Producer<GraphUpdate>,
    _stream: cpal::Stream,
}

impl Engine {
    pub fn new(project: Arc<ProjectData>) -> Result<Self> {
        use cpal::traits::StreamTrait;

        let config = EngineConfig::create()?;
        let schedule = project.compile_graph()?;
        assert!(
            !(schedule.buffer_count > MAX_BUFFER_SLOTS || project.graph.nodes.len() > MAX_NODES),
            "Graph is too large"
        );

        // Initial state for every node already in the fresh graph.
        let state_additions: Vec<_> = project
            .graph
            .nodes
            .iter()
            .map(|(id, node)| (id, node.spawn_state()))
            .collect();

        let (mut update_tx, update_rx) = rtrb::RingBuffer::<GraphUpdate>::new(UPDATE_RING_CAPACITY);
        let (garbage_tx, mut garbage_rx) = rtrb::RingBuffer::<Garbage>::new(GARBAGE_RING_CAPACITY);

        // Seed the ring with the initial graph so the audio thread has
        // something to play from the very first callback.
        let _ = update_tx.push(GraphUpdate {
            project: project.clone(),
            schedule: Arc::new(schedule),
            state_additions,
            state_removals: Vec::new(),
        });

        // Background thread: the only place anything from the audio thread
        // actually gets dropped/deallocated.
        std::thread::spawn(move || {
            loop {
                while let Ok(garbage) = garbage_rx.pop() {
                    drop(garbage);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });

        let transport = Arc::new(Transport::default());
        let playhead = Arc::new(AtomicU64::new(0));

        let stream = Self::build_stream::<f32>(
            &config,
            transport.clone(),
            playhead.clone(),
            update_rx,
            garbage_tx,
        )?;
        stream.play()?; // device stream runs continuously; transport gates output

        Ok(Self {
            config,
            playhead,
            transport,
            current: project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            audio_manager: AudioManager {
                update_tx,
                _stream: stream,
            },
        })
    }

    pub fn project(&self) -> &ProjectData {
        &self.current
    }

    pub const fn sample_rate(&self) -> u32 {
        self.config.config.sample_rate
    }

    pub const fn channels(&self) -> u16 {
        self.config.config.channels
    }

    /// Not a Command on purpose — asset import is I/O-bound and, unlike
    /// graph/clip edits, isn't meaningfully undo-able in the same sense.
    /// A real engine would still route this through some queue so it
    /// doesn't block the caller, but it's direct here for clarity.
    pub fn load_asset(&mut self, path: impl Into<PathBuf>) -> Result<AudioAssetID> {
        let asset = assetserver::load_audio_asset(path, self.sample_rate())?;
        let mut next = (*self.current).clone();
        let id = next.assets.insert(asset);
        self.commit(next);
        Ok(id)
    }

    pub fn apply<T>(&mut self, cmd: T) -> Result<T::Output>
    where
        T: Command,
    {
        let mut next = (*self.current).clone();

        let res = Self::apply_command(&mut next, cmd)?;
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

    /// Builds the next `GraphUpdate` off the audio thread and pushes it
    /// through the ring. Allocation happens here, on the control thread —
    /// that's fine, this is not the real-time path.
    fn publish_current(&mut self) {
        let schedule = self
            .current
            .compile_graph()
            .expect("command validation prevents cycles");

        if schedule.buffer_count > MAX_BUFFER_SLOTS || self.current.graph.nodes.len() > MAX_NODES {
            // In a real UI this would surface as a rejected edit before
            // getting here (validate in Command::execute); this is the
            // last-resort backstop.
            eprintln!("graph exceeds preallocated real-time budget; edit ignored");
            return;
        }

        let old_ids: std::collections::HashSet<NodeID> = self
            .undo_stack
            .last()
            .map(|proj| proj.graph.nodes.keys().collect())
            .unwrap_or_default();
        let new_ids: std::collections::HashSet<NodeID> = self.current.graph.nodes.keys().collect();

        let state_additions: Vec<_> = new_ids
            .difference(&old_ids)
            .map(|&id| (id, self.current.graph.nodes[id].spawn_state()))
            .collect();
        let state_removals: Vec<_> = old_ids.difference(&new_ids).copied().collect();

        let update = GraphUpdate {
            project: self.current.clone(),
            schedule: Arc::new(schedule),
            state_additions,
            state_removals,
        };

        if self.audio_manager.update_tx.push(update).is_err() {
            eprintln!("update ring full — audio thread stalled or edits too rapid; dropping edit");
        }
    }

    fn apply_command<T>(project: &mut ProjectData, cmd: T) -> Result<T::Output>
    where
        T: Command,
    {
        cmd.execute(project)
    }

    pub fn move_playhead(&self, to: Tick) -> Result<()> {
        self.playhead.swap(to.0, Ordering::Relaxed);
        Ok(())
    }

    fn build_stream<T>(
        config: &EngineConfig,
        transport: Arc<Transport>,
        playhead: Arc<AtomicU64>,
        mut update_rx: rtrb::Consumer<GraphUpdate>,
        mut garbage_tx: rtrb::Producer<Garbage>,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        use cpal::traits::DeviceTrait;
        let channels = config.config.channels;
        let device = config.device.clone();
        let mut buffer_pool = BlockBufferPool::new(MAX_BUFFER_SLOTS, 1024);

        let mut state_pool = NodeStatePool::new();
        let mut current: Option<GraphUpdate> = None;
        let stream = device.build_output_stream(
            config.config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                assert_no_alloc::assert_no_alloc(|| {
                    data.fill(T::from_sample(0.0));
                    // Tier 1: drain any pending structural updates. Zero
                    // allocation: everything was pre-built off-thread.
                    while let Ok(mut update) = update_rx.pop() {
                        state_pool.apply(&mut update, &mut garbage_tx);
                        if let Some(old) = current.replace(update) {
                            let _ = garbage_tx.push(Garbage::Update(old));
                        }
                    }

                    let frame_count = data.len() / channels as usize;
                    let start = playhead.fetch_add(frame_count as u64, Ordering::Relaxed);

                    if !transport.is_playing() {
                        return;
                    }

                    let Some(GraphUpdate {
                        project, schedule, ..
                    }) = current.as_ref()
                    else {
                        return;
                    };

                    let mixed = execute_block(
                        schedule,
                        project,
                        Tick(start),
                        &mut buffer_pool,
                        &mut state_pool,
                    );

                    for (dst, &src) in data.iter_mut().zip(mixed) {
                        *dst = T::from_sample(src);
                    }
                });
            },
            move |err| eprintln!("audio stream error: {err}"),
            None,
        )?;

        Ok(stream)
    }
}
