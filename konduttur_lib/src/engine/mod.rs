pub mod assetserver;
pub mod bbp;
pub mod command;
pub mod engineconfig;
pub mod state;
pub mod tick;

use std::any::Any;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, atomic::AtomicU64};

use crate::engine::bbp::BlockBufferPool;
pub use crate::engine::command::*;
use crate::engine::state::{
    GARBAGE_RING_CAPACITY, Garbage, GraphUpdate, MAX_NODES, NodeStatePool, UPDATE_RING_CAPACITY,
};
use crate::engine::{engineconfig::EngineConfig, tick::Tick};
use crate::model::{
    DataKind,
    asset::{AudioAsset, AudioAssetID},
    flow::NodeID,
    project::ProjectData,
};

use anyhow::Result;
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
    config: &EngineConfig,
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
            state_pool.get_mut(step.node_id),
            block_start,
            config,
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

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum TransportState {
    Stopped,
    Playing,
    Recording,
    Paused,
}

#[derive(Debug, Default)]
pub struct Transport(AtomicU8);

impl Transport {
    pub fn new() -> Self {
        Self(AtomicU8::new(0))
    }
    fn transport(&self, to: TransportState) {
        self.0.store(to as u8, Ordering::Relaxed);
    }

    pub fn play(&self) {
        self.transport(TransportState::Playing);
    }

    pub fn stop(&self) {
        self.transport(TransportState::Stopped);
    }

    #[inline]
    pub fn is_playing(&self) -> bool {
        self.0.load(Ordering::Relaxed) == TransportState::Playing as u8
    }
}

pub struct Engine {
    pub transport: Arc<Transport>,
    pub playhead: Arc<AtomicU64>,
    config: EngineConfig,
    current: Arc<ProjectData>,
    undo_stack: Vec<Arc<ProjectData>>,
    redo_stack: Vec<Arc<ProjectData>>,
    update_tx: rtrb::Producer<GraphUpdate>,
    _stream: cpal::Stream,
}

pub const MAX_BUFFER_SLOTS: usize = 4096;
impl Engine {
    pub fn new(project: Arc<ProjectData>) -> Result<Self> {
        use cpal::traits::{DeviceTrait, StreamTrait};

        let config = EngineConfig::create()?;
        let channels = config.config.channels;
        let schedule = project
            .compile_graph()
            .expect("fresh project graph is always acyclic");
        if schedule.buffer_count > MAX_BUFFER_SLOTS || project.graph.nodes.len() > MAX_NODES {
            panic!("Graph is too large");
        }

        // Initial state for every node already in the fresh graph.
        let state_additions: Vec<_> = project
            .graph
            .nodes
            .iter()
            .map(|(id, node)| (id, node.init_state(channels)))
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
        let playhead = Arc::new(std::sync::atomic::AtomicU64::new(0));

        let stream = Self::build_stream::<f32>(
            config.clone(),
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
            update_tx,
            _stream: stream,
        })
    }

    pub fn project(&self) -> &ProjectData {
        &self.current
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.config.sample_rate
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

    /// Builds the next GraphUpdate off the audio thread and pushes it
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
            .map(|p| p.graph.nodes.keys().collect())
            .unwrap_or_default();
        let new_ids: std::collections::HashSet<NodeID> = self.current.graph.nodes.keys().collect();

        let state_additions: Vec<_> = new_ids
            .difference(&old_ids)
            .map(|&id| {
                (
                    id,
                    self.current.graph.nodes[id].init_state(self.config.config.channels),
                )
            })
            .collect();
        let state_removals: Vec<_> = old_ids.difference(&new_ids).copied().collect();

        let update = GraphUpdate {
            project: self.current.clone(),
            schedule: Arc::new(schedule),
            state_additions,
            state_removals,
        };

        if self.update_tx.push(update).is_err() {
            eprintln!("update ring full — audio thread stalled or edits too rapid; dropping edit");
        }
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

    fn build_stream<T>(
        config: EngineConfig,
        transport: Arc<Transport>,
        playhead: Arc<std::sync::atomic::AtomicU64>,
        mut update_rx: rtrb::Consumer<GraphUpdate>,
        mut garbage_tx: rtrb::Producer<Garbage>,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        use cpal::traits::DeviceTrait;

        let device = config.device.clone();
        let mut buffer_pool = BlockBufferPool::new(MAX_BUFFER_SLOTS, 1024);
        let mut state_pool = NodeStatePool::new();
        let mut current: Option<GraphUpdate> = None;

        let stream = device.build_output_stream(
            config.config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                assert_no_alloc::assert_no_alloc(|| {
                    // Tier 1: drain any pending structural updates. Zero
                    // allocation: everything was pre-built off-thread.
                    while let Ok(mut update) = update_rx.pop() {
                        state_pool.apply(&mut update, &mut garbage_tx);
                        if let Some(old) = current.replace(update) {
                            let _ = garbage_tx.push(Garbage::Update(old));
                        }
                    }

                    let frame_count = data.len() / config.config.channels as usize;
                    let start = playhead.fetch_add(frame_count as u64, Ordering::Relaxed);

                    if !transport.is_playing() {
                        data.fill(T::from_sample(0.0));
                        return;
                    }

                    let Some(GraphUpdate {
                        project, schedule, ..
                    }) = current.as_ref()
                    else {
                        data.fill(T::from_sample(0.0));
                        return;
                    };

                    let mixed = execute_block(
                        schedule,
                        project,
                        Tick(start),
                        &config,
                        &mut buffer_pool,
                        &mut state_pool,
                    );

                    for (dst, &src) in data.iter_mut().zip(mixed) {
                        *dst = T::from_sample(src);
                    }
                })
            },
            move |err| eprintln!("audio stream error: {err}"),
            None,
        )?;

        Ok(stream)
    }
}
