use std::collections::{HashMap, VecDeque};

use crate::{
    engine::{CompiledGraph, EngineError, ScheduleStep, tick::Tick},
    model::{
        DataKind,
        arr::{
            clip::{Clip, ClipData, ClipID},
            track::{Track, TrackID},
        },
        asset::{Asset, AssetID},
        flow::{Link, NativeNodeType, Node, NodeGraph, NodeID, NodePayload, Socket, SocketIndex},
    },
};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub tracks: SlotMap<TrackID, Track>,
    pub clips: SlotMap<ClipID, Clip>,
    pub assets: SlotMap<AssetID, Asset>,
    pub graph: NodeGraph,
    pub master_node_id: NodeID,
}

impl Project {
    pub fn new() -> Self {
        let mut graph = NodeGraph::default();

        let master_node = Node::new(
            vec![Socket::new(DataKind::Audio, "in", true)],
            vec![Socket::new(DataKind::Audio, "out", false)],
            NodePayload::Native(NativeNodeType::Master),
        );
        let master_node_id = graph.nodes.insert(master_node);
        Self {
            tracks: SlotMap::with_key(),
            clips: SlotMap::with_key(),
            assets: SlotMap::with_key(),
            graph,
            master_node_id,
        }
    }

    pub fn remove_link(
        &mut self,
        from: (NodeID, u16),
        to: (NodeID, u16),
    ) -> Result<(), EngineError> {
        self.graph
            .links
            .retain(|_, l| !(l.from == from && l.to == to));
        Ok(())
    }

    pub fn add_link(&mut self, from: (NodeID, u16), to: (NodeID, u16)) -> Result<(), EngineError> {
        let from_kind = self.socket_kind_of(from, true)?;
        let to_kind = self.socket_kind_of(to, false)?;
        if !from_kind.can_connect_to(to_kind) {
            return Err(EngineError::IncompatibleSockets {
                from: from_kind,
                to: to_kind,
            });
        }
        self.graph.links.insert(Link { from, to });
        if self.topo_sort().is_err() {
            self.graph
                .links
                .retain(|_, l| !(l.from == from && l.to == to));
            return Err(EngineError::WouldCreateCycle);
        }
        Ok(())
    }

    pub fn move_clip(
        &mut self,
        track: TrackID,
        clip: ClipID,
        new_start: Tick,
    ) -> Result<(), EngineError> {
        let track = self
            .tracks
            .get_mut(track)
            .ok_or(EngineError::TrackNotFound(track))?;
        track.clips.retain(|_, &mut id| id != clip);
        track.clips.insert(new_start, clip);
        if let Some(c) = self.clips.get_mut(clip) {
            c.start = new_start;
        }
        Ok(())
    }

    pub fn add_clip_to_track(
        &mut self,
        track: TrackID,
        start: Tick,
        length: Tick,
        asset: AssetID,
    ) -> Result<(), EngineError> {
        let clip_id = self.clips.insert(Clip {
            start,
            length,
            data: ClipData::Audio(asset),
        });
        let track = self
            .tracks
            .get_mut(track)
            .ok_or(EngineError::TrackNotFound(track))?;
        track.clips.insert(start, clip_id);
        Ok(())
    }

    pub fn remove_track(&mut self, track_id: TrackID) -> Result<(), EngineError> {
        let track = self
            .tracks
            .remove(track_id)
            .ok_or(EngineError::TrackNotFound(track_id))?;
        let linked_id = track.linked_node_id.expect("Track was orphaned from node");
        self.graph.nodes.remove(linked_id);
        self.graph
            .links
            .retain(|_, l| l.from.0 != linked_id && l.to.0 != linked_id);
        for clip_id in track.clips.values() {
            self.clips.remove(*clip_id);
        }
        Ok(())
    }

    pub fn add_track(&mut self, name: String, kind: DataKind) -> Result<(), EngineError> {
        let track_id = self.tracks.insert(Track {
            name,
            kind,
            gain: 1.0,
            linked_node_id: None,
            clips: Default::default(),
        });
        let node = Node::new(
            vec![],
            vec![Socket::new(kind, "out", true)],
            NodePayload::TrackReader(track_id),
        );
        let node_id = self.graph.nodes.insert(node);
        self.tracks[track_id].linked_node_id = Some(node_id);

        // Convenience default: new tracks route straight to master.
        // Same AddLink path a user rewiring Flow view would go through.
        let master = self.master_node_id;
        self.graph.links.insert(Link {
            from: (node_id, 0),
            to: (master, 0),
        });
        Ok(())
    }

    pub fn socket_kind_of(
        &self,
        endpoint: (NodeID, SocketIndex),
        is_output: bool,
    ) -> Result<DataKind, EngineError> {
        let node = self
            .graph
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

    /// Kahn's algorithm topological sort + a bump-allocated buffer slot per
    /// output socket. No slot reuse yet (see the buffer-pooling discussion —
    /// this is the register-allocation pass that'd go here later); today's
    /// graphs are tiny so it doesn't matter yet.
    /// Kahn's algorithm, shared by compile_graph (which needs the order) and
    /// link validation (which only needs to know whether an order exists).
    fn topo_sort(&self) -> Result<Vec<NodeID>, EngineError> {
        let mut in_degree: HashMap<NodeID, usize> =
            self.graph.nodes.keys().map(|id| (id, 0)).collect();
        for link in self.graph.links.values() {
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
        let mut order = Vec::with_capacity(self.graph.nodes.len());

        while let Some(n) = queue.pop_front() {
            order.push(n);
            for link in self.graph.links.values().filter(|l| l.from.0 == n) {
                let d = remaining.get_mut(&link.to.0).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(link.to.0);
                }
            }
        }
        if order.len() != self.graph.nodes.len() {
            return Err(EngineError::WouldCreateCycle);
        }
        Ok(order)
    }

    pub fn compile_graph(&self) -> Result<CompiledGraph, EngineError> {
        let order = self.topo_sort()?;

        let mut output_slot: HashMap<(NodeID, SocketIndex), usize> = HashMap::new();
        let mut buffer_count = 0usize;
        for &node_id in &order {
            let node = &self.graph.nodes[node_id];
            for i in 0..node.outputs.len() {
                output_slot.insert((node_id, i as SocketIndex), buffer_count);
                buffer_count += 1;
            }
        }

        let mut steps = Vec::with_capacity(order.len());
        for &node_id in &order {
            let node = &self.graph.nodes[node_id];
            let mut input_sources = vec![Vec::new(); node.inputs.len()];
            for link in self.graph.links.values().filter(|l| l.to.0 == node_id) {
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
            .get(&(self.master_node_id, 0))
            .ok_or(EngineError::NodeNotFound(self.master_node_id))?;

        Ok(CompiledGraph {
            steps,
            buffer_count,
            master_output_slot,
        })
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}
