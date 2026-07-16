use arc_swap::ArcSwap;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, OnceLock};
use thiserror::Error;
pub mod assetserver;

use crate::model::{
    DataKind,
    arr::{
        clip::{Clip, ClipData, ClipID},
        track::{Track, TrackID},
    },
    asset::{Asset, AssetID},
    flow::{Link, NativeNodeType, Node, NodeGraph, NodeID, NodePayload, Socket, SocketIndex},
    project::Project,
};

/// Atomic unit of time within the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick(pub u64);

impl From<u64> for Tick {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<usize> for Tick {
    fn from(value: usize) -> Self {
        Self(value as u64)
    }
}

impl std::ops::Add for Tick {
    type Output = Tick;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Tick {
    type Output = Tick;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

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

/// Kahn's algorithm topological sort + a bump-allocated buffer slot per
/// output socket. No slot reuse yet (see the buffer-pooling discussion —
/// this is the register-allocation pass that'd go here later); today's
/// graphs are tiny so it doesn't matter yet.
/// Kahn's algorithm, shared by compile_graph (which needs the order) and
/// link validation (which only needs to know whether an order exists).
fn topo_sort(graph: &NodeGraph) -> Result<Vec<NodeID>, EngineError> {
    let mut in_degree: HashMap<NodeID, usize> = graph.nodes.keys().map(|id| (id, 0)).collect();
    for link in graph.links.values() {
        *in_degree
            .get_mut(&link.to.0)
            .ok_or(EngineError::NodeNotFound(link.to.0))? += 1;
    }

    let mut remaining = in_degree.clone();
    let mut queue: VecDeque<NodeID> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut order = Vec::with_capacity(graph.nodes.len());

    while let Some(n) = queue.pop_front() {
        order.push(n);
        for link in graph.links.values().filter(|l| l.from.0 == n) {
            let d = remaining.get_mut(&link.to.0).unwrap();
            *d -= 1;
            if *d == 0 {
                queue.push_back(link.to.0);
            }
        }
    }
    if order.len() != graph.nodes.len() {
        return Err(EngineError::WouldCreateCycle);
    }
    Ok(order)
}

pub fn compile_graph(graph: &NodeGraph, master_node: NodeID) -> Result<CompiledGraph, EngineError> {
    let order = topo_sort(graph)?;

    let mut output_slot: HashMap<(NodeID, SocketIndex), usize> = HashMap::new();
    let mut buffer_count = 0usize;
    for &node_id in &order {
        let node = &graph.nodes[node_id];
        for i in 0..node.outputs.len() {
            output_slot.insert((node_id, i as SocketIndex), buffer_count);
            buffer_count += 1;
        }
    }

    let mut steps = Vec::with_capacity(order.len());
    for &node_id in &order {
        let node = &graph.nodes[node_id];
        let mut input_sources = vec![Vec::new(); node.inputs.len()];
        for link in graph.links.values().filter(|l| l.to.0 == node_id) {
            if let Some(&slot) = output_slot.get(&link.from) {
                input_sources[link.to.1 as usize].push(slot);
            }
        }
        let output_slots = (0..node.outputs.len())
            .map(|i| output_slot[&(node_id, i as SocketIndex)])
            .collect();
        steps.push(ScheduleStep {
            node: node_id,
            input_sources,
            output_slots,
        });
    }

    let master_output_slot = *output_slot
        .get(&(master_node, 0))
        .ok_or(EngineError::NodeNotFound(master_node))?;

    Ok(CompiledGraph {
        steps,
        buffer_count,
        master_output_slot,
    })
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
                track.render_into_buf(&project, &mut outputs[0], block_start, channels);
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
        length: Tick,
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
    RemoveLink(NodeID, SocketIndex, NodeID, SocketIndex),
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
        let schedule = compile_graph(&project.graph, project.master_node_id)
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
        let schedule = compile_graph(&self.current.graph, self.current.master_node_id)
            .expect("command validation should prevent cycles before this point");
        self.publish.store(Arc::new(RenderState {
            project: self.current.clone(),
            schedule: Arc::new(schedule),
        }));
    }
}

fn apply_command(project: &mut Project, cmd: Command) -> Result<(), EngineError> {
    match cmd {
        Command::AddTrack { name, kind } => add_track(project, name, kind),
        Command::RemoveTrack(track_id) => remove_track(project, track_id),
        Command::AddClip {
            track,
            start,
            length,
            asset,
        } => add_clip_to_track(project, track, start, length, asset),
        Command::MoveClip {
            track,
            clip,
            new_start,
        } => move_clip(project, track, clip, new_start),
        Command::AddLink { from, to } => add_link(project, from, to),
        Command::RemoveLink(fn_, fs, tn, ts) => remove_link(project, fn_, fs, tn, ts),
    }
}

fn remove_link(
    project: &mut Project,
    fn_: NodeID,
    fs: u16,
    tn: NodeID,
    ts: u16,
) -> Result<(), EngineError> {
    project
        .graph
        .links
        .retain(|_, l| !(l.from == (fn_, fs) && l.to == (tn, ts)));
    Ok(())
}

fn add_link(
    project: &mut Project,
    from: (NodeID, u16),
    to: (NodeID, u16),
) -> Result<(), EngineError> {
    let from_kind = socket_kind_of(&project.graph, from, true)?;
    let to_kind = socket_kind_of(&project.graph, to, false)?;
    if !from_kind.can_connect_to(to_kind) {
        return Err(EngineError::IncompatibleSockets {
            from: from_kind,
            to: to_kind,
        });
    }
    project.graph.links.insert(Link { from, to });
    if topo_sort(&project.graph).is_err() {
        project
            .graph
            .links
            .retain(|_, l| !(l.from == from && l.to == to));
        return Err(EngineError::WouldCreateCycle);
    }
    Ok(())
}

fn move_clip(
    project: &mut Project,
    track: TrackID,
    clip: ClipID,
    new_start: Tick,
) -> Result<(), EngineError> {
    let track = project
        .tracks
        .get_mut(track)
        .ok_or(EngineError::TrackNotFound(track))?;
    track.clips.retain(|_, &mut id| id != clip);
    track.clips.insert(new_start, clip);
    if let Some(c) = project.clips.get_mut(clip) {
        c.start = new_start;
    }
    Ok(())
}

fn add_clip_to_track(
    project: &mut Project,
    track: TrackID,
    start: Tick,
    length: Tick,
    asset: AssetID,
) -> Result<(), EngineError> {
    let clip_id = project.clips.insert(Clip {
        start,
        length,
        data: ClipData::Audio(asset),
    });
    let track = project
        .tracks
        .get_mut(track)
        .ok_or(EngineError::TrackNotFound(track))?;
    track.clips.insert(start, clip_id);
    Ok(())
}

fn remove_track(project: &mut Project, track_id: TrackID) -> Result<(), EngineError> {
    let track = project
        .tracks
        .remove(track_id)
        .ok_or(EngineError::TrackNotFound(track_id))?;
    let linked_id = *track.linked_node_id.get().unwrap();
    project.graph.nodes.remove(linked_id);
    project
        .graph
        .links
        .retain(|_, l| l.from.0 != linked_id && l.to.0 != linked_id);
    for clip_id in track.clips.values() {
        project.clips.remove(*clip_id);
    }
    Ok(())
}

fn add_track(project: &mut Project, name: String, kind: DataKind) -> Result<(), EngineError> {
    let track_id = project.tracks.insert(Track {
        name,
        kind,
        gain: 1.0,
        linked_node_id: OnceLock::new(),
        clips: Default::default(),
    });
    let node = Node::new(
        vec![],
        vec![Socket::new(kind, "out")],
        NodePayload::TrackReader(track_id),
    );
    let node_id = project.graph.nodes.insert(node);
    project.tracks[track_id].linked_node_id.set(node_id);
    // Convenience default: new tracks route straight to master.
    // Same AddLink path a user rewiring Flow view would go through.
    let master = project.master_node_id;
    project.graph.links.insert(Link {
        from: (node_id, 0),
        to: (master, 0),
    });
    Ok(())
}

fn socket_kind_of(
    graph: &NodeGraph,
    endpoint: (NodeID, SocketIndex),
    is_output: bool,
) -> Result<DataKind, EngineError> {
    let node = graph
        .nodes
        .get(endpoint.0)
        .ok_or(EngineError::NodeNotFound(endpoint.0))?;
    let list = if is_output {
        &node.outputs
    } else {
        &node.inputs
    };
    list.get(endpoint.1 as usize)
        .map(|s| s.kind)
        .ok_or(EngineError::NodeNotFound(endpoint.0))
}
