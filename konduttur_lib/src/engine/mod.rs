use arc_swap::ArcSwap;
use std::sync::Arc;
use thiserror::Error;
pub mod assetserver;
pub mod tick;

use crate::engine::tick::Tick;
use crate::model::Renderable;
use crate::model::{
    DataKind,
    arr::{clip::ClipID, track::TrackID},
    asset::{Asset, AssetID},
    flow::{NativeNodeType, NodeID, NodePayload, SocketIndex},
    project::Project,
};

// ---------------------------------------------------------------------
// Compiled schedule
// ---------------------------------------------------------------------

pub struct ScheduleStep {
    pub node: NodeID,
    /// One entry per input socket; each entry lists every buffer slot
    /// feeding it (0 = unconnected, 1 = normal, 2+ = fan-in summed).
    pub input_sources: Vec<Vec<usize>>,
    /// One entry per output socket.
    pub output_slots: Vec<usize>,
}

pub struct CompiledGraph {
    pub steps: Vec<ScheduleStep>,
    pub buffer_count: usize,
    pub master_output_slot: usize,
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

/// Scratch memory reused across callbacks. Currently reallocates its inner
/// per-node Vecs each block (see process_node's `Vec::new()` scratch) —
/// flagged as the real-time-safety TODO it is; fine for a skeleton, not
/// for production. Real version preallocates every scratch slot exactly
/// once, sized when the schedule is published.
pub struct BufferPool {
    buffers: Vec<Vec<f32>>,
}

impl BufferPool {
    pub fn new() -> Self {
        Self {
            buffers: Vec::new(),
        }
    }

    fn ensure(&mut self, buffer_count: usize, frame_capacity: usize) {
        if self.buffers.len() != buffer_count {
            self.buffers = vec![vec![0.0; frame_capacity]; buffer_count];
        }
        for buf in &mut self.buffers {
            if buf.len() < frame_capacity {
                buf.resize(frame_capacity, 0.0);
            }
        }
    }
}

/// Runs the compiled schedule for one block and returns the master mix.
pub fn execute_block<'a>(
    schedule: &CompiledGraph,
    project: &Project,
    block_start: Tick,
    frame_count: usize,
    channels: u16,
    pool: &'a mut BufferPool,
) -> &'a [f32] {
    let sample_count = frame_count * channels as usize;
    pool.ensure(schedule.buffer_count, sample_count);

    for step in &schedule.steps {
        // Gather + sum inputs (audio/CV fan-in) into owned scratch first,
        // so we never hold an immutable borrow of pool.buffers while also
        // writing this node's outputs into it below.
        let mut inputs: Vec<Vec<f32>> = Vec::with_capacity(step.input_sources.len());
        for sources in &step.input_sources {
            let mut acc = vec![0.0f32; sample_count];
            for &slot in sources {
                for (a, b) in acc.iter_mut().zip(&pool.buffers[slot][..sample_count]) {
                    *a += b;
                }
            }
            inputs.push(acc);
        }

        let mut outputs: Vec<Vec<f32>> = step
            .output_slots
            .iter()
            .map(|_| vec![0.0f32; sample_count])
            .collect();

        let node = &project.graph.nodes[step.node];
        process_node(
            &node.payload,
            project,
            block_start,
            channels,
            &inputs,
            &mut outputs,
        );

        for (&slot, buf) in step.output_slots.iter().zip(outputs) {
            pool.buffers[slot][..sample_count].copy_from_slice(&buf);
        }
    }

    &pool.buffers[schedule.master_output_slot][..sample_count]
}

fn process_node(
    payload: &NodePayload,
    project: &Project,
    block_start: Tick,
    channels: u16,
    inputs: &[Vec<f32>],
    outputs: &mut [Vec<f32>],
) {
    match payload {
        NodePayload::TrackReader(track_id) => {
            if let Some(track) = project.tracks.get(*track_id) {
                track.render(project, &mut outputs[0], block_start, channels);
            }
        }
        NodePayload::Native(NativeNodeType::Master) => {
            outputs[0].copy_from_slice(&inputs[0]);
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
        }
    }
}

/// What the audio thread reads. Today this is "the whole Project plus its
/// compiled schedule"; splitting Project into a separately-published
/// TimelineSnapshot (per the earlier design) is a later optimization that
/// doesn't change anything on the Engine/Command side.
pub struct RenderState {
    pub project: Arc<Project>,
    pub schedule: Arc<CompiledGraph>,
}

pub struct Engine {
    current: Arc<Project>,
    undo_stack: Vec<Arc<Project>>,
    redo_stack: Vec<Arc<Project>>,
    publish: Arc<ArcSwap<RenderState>>,
}

impl Engine {
    pub fn new(project: Arc<Project>) -> Self {
        let schedule = project
            .compile_graph()
            .expect("fresh project graph is always acyclic");

        let publish = Arc::new(ArcSwap::from_pointee(RenderState {
            project: project.clone(),
            schedule: Arc::new(schedule),
        }));
        Self {
            current: project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            publish,
        }
    }

    /// Clone this and hand it to the audio callback; it's the read side of
    /// the lock-free publish.
    pub fn render_state_handle(&self) -> Arc<ArcSwap<RenderState>> {
        self.publish.clone()
    }

    pub fn project(&self) -> &Project {
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

    pub fn apply(&mut self, cmd: Command) -> Result<(), EngineError> {
        let mut next = (*self.current).clone();
        println!("{cmd}");
        apply_command(&mut next, cmd)?;
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

    fn commit(&mut self, next: Project) {
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
}

fn apply_command(project: &mut Project, cmd: Command) -> Result<(), EngineError> {
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
    }
}
