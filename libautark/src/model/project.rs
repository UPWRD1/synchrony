use std::collections::{HashMap, VecDeque};

use crate::{
    engine::{CompiledGraph, EngineError, ScheduleStep, SlotIndex, tick::Tick},
    model::{
        DataKind, Kind, Stored,
        arr::{
            clip::{AudioClip, AudioClipID, Clip},
            track::{AudioTrack, AudioTrackID, Track},
        },
        asset::{AudioAsset, AudioAssetID},
        flow::{
            Link, Node, NodeGraph, NodeID, SocketIndex,
            nodes::{master::Master, trackreader::TrackReader},
        },
    },
};
use anyhow::Result;

use slotmap::SlotMap;

#[derive(Debug, Clone)]
pub struct ProjectData {
    pub tracks: SlotMap<AudioTrackID, AudioTrack>,
    pub clips: SlotMap<AudioClipID, AudioClip>,
    pub assets: SlotMap<AudioAssetID, AudioAsset>,
    pub graph: NodeGraph,
    pub master_node_id: NodeID,
}

impl ProjectData {
    pub fn new() -> Self {
        let mut graph = NodeGraph::default();

        let master_node = Master;
        let master_node_id = graph.nodes.insert(Box::new(master_node));
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
        from: (NodeID, SocketIndex),
        to: (NodeID, SocketIndex),
    ) -> Result<()> {
        self.graph.links.retain(|l| !(l.from == from && l.to == to));
        Ok(())
    }

    pub fn add_link(
        &mut self,
        from: (NodeID, SocketIndex),
        to: (NodeID, SocketIndex),
    ) -> Result<Option<Link>> {
        let from_kind = self.socket_kind_of(from, true)?;
        let to_kind = self.socket_kind_of(to, false)?;
        if !from_kind.can_connect_to(to_kind) {
            return Err(EngineError::IncompatibleSockets {
                from: from_kind,
                to: to_kind,
            }
            .into());
        }
        let link = Link { from, to };
        // Prevent multiple inputs into the same slot
        let prev_link = self.graph.links.replace(link);

        if self.topo_sort().is_err() {
            self.graph.links.retain(|l| !(l.from == from && l.to == to));
            return Err(EngineError::WouldCreateCycle.into());
        }
        Ok(prev_link)
    }

    pub fn move_clip<K: Kind>(
        &mut self,
        track: <K::Track as Stored>::Id,
        clip: <K::Clip as Stored>::Id,
        new_start: Tick,
    ) -> Result<()> {
        let track = K::Track::access_mut(self)
            .get_mut(track)
            .ok_or(EngineError::TrackNotFound)?;
        track.clips_mut().retain(|_, &mut id| id != clip);
        track.clips_mut().insert(new_start, clip);
        if let Some(c) = K::Clip::access_mut(self).get_mut(clip) {
            *c.start_mut() = new_start;
        }
        Ok(())
    }

    pub fn add_clip_to_track<K: Kind>(
        &mut self,
        track: <K::Track as Stored>::Id,
        start: Tick,
        length: Tick,
        asset_id: <K::Asset as Stored>::Id,
    ) -> Result<<K::Clip as Stored>::Id> {
        let clip_id = K::Clip::access_mut(self).insert(K::Clip::new(start, length, asset_id));
        let track = K::Track::access_mut(self)
            .get_mut(track)
            .ok_or(EngineError::TrackNotFound)?;
        track.clips_mut().insert(start, clip_id);
        Ok(clip_id)
    }

    pub fn remove_track<K: Kind>(&mut self, track_id: <K::Track as Stored>::Id) -> Result<()> {
        let track = K::Track::access_mut(self)
            .remove(track_id)
            .ok_or(EngineError::TrackNotFound)?;
        let linked_id = track
            .linked_node_id()
            .expect("Track was orphaned from node");
        self.graph.nodes.remove(linked_id);
        self.graph
            .links
            .retain(|l| l.from.0 != linked_id && l.to.0 != linked_id);
        for clip_id in track.clips().values() {
            K::Clip::access_mut(self).remove(*clip_id);
        }
        Ok(())
    }

    pub fn add_track<K: Kind>(
        &mut self,
        name: String,
        channels: u16,
    ) -> Result<(<K::Track as Stored>::Id, NodeID)>
    where
        TrackReader<K>: Node,
    {
        let track_id = K::Track::access_mut(self).insert(K::Track::new(name));
        let reader_node = TrackReader::<K>::new(track_id, channels);
        let node_id = self.graph.nodes.insert(Box::new(reader_node));
        *K::Track::access_mut(self)[track_id].linked_node_id_mut() = Some(node_id);
        Ok((track_id, node_id))
    }

    pub fn add_node<N: Node>(&mut self, node: N) -> NodeID {
        self.graph.nodes.insert(Box::new(node))
    }

    pub fn socket_kind_of(
        &mut self,
        endpoint: (NodeID, SocketIndex),
        is_output: bool,
    ) -> Result<DataKind> {
        let node = self
            .graph
            .nodes
            .get_mut(endpoint.0)
            .ok_or(EngineError::NodeNotFound(endpoint.0))?;
        if is_output {
            let list = node.outputs();
            list.get(endpoint.1).map(|s| &s.kind).cloned()
        } else {
            let input = node.input(endpoint.1);
            input.map(|s| s.kind)
        }
        .ok_or(EngineError::SocketNotFound(endpoint.0, endpoint.1).into())
    }

    /// Kahn's algorithm topological sort + a bump-allocated buffer slot per
    /// output socket. No slot reuse yet (see the buffer-pooling discussion —
    /// this is the register-allocation pass that'd go here later); today's
    /// graphs are tiny so it doesn't matter yet.
    /// Kahn's algorithm, shared by compile_graph (which needs the order) and
    /// link validation (which only needs to know whether an order exists).
    fn topo_sort(&self) -> Result<Vec<NodeID>> {
        let mut in_degree: HashMap<NodeID, usize> =
            self.graph.nodes.keys().map(|id| (id, 0)).collect();

        for link in self.graph.links.iter() {
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
            for link in self.graph.links.iter().filter(|l| l.from.0 == n) {
                let d = remaining.get_mut(&link.to.0).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(link.to.0);
                }
            }
        }
        if order.len() != self.graph.nodes.len() {
            return Err(EngineError::WouldCreateCycle.into());
        }
        Ok(order)
    }

    pub fn compile_graph(&self) -> Result<CompiledGraph> {
        let order = self.topo_sort()?;

        // Tracks output socket -> physical layout buffer slot mapping
        let mut output_slot: HashMap<(NodeID, SocketIndex), SlotIndex> = HashMap::new();

        // Slot 0 is permanently reserved for Silence/Unconnected states.
        // Node outputs start assigning from Slot index 1.
        let mut buffer_count = 1usize;

        for &node_id in &order {
            let node = &self.graph.nodes[node_id];
            for i in 0..node.outputs().len() {
                output_slot.insert((node_id, i as SocketIndex), buffer_count);
                buffer_count += 1;
            }
        }

        let mut steps = Vec::with_capacity(order.len());

        for &node_id in &order {
            let node = &self.graph.nodes[node_id];

            // Temporary container to collect all lines feeding each input socket
            let mut raw_input_sources = vec![Vec::new(); node.inputs().len()];
            for link in self.graph.links.iter().filter(|l| l.to.0 == node_id) {
                if let Some(&slot) = output_slot.get(&link.from) {
                    raw_input_sources[link.to.1].push(slot);
                }
            }

            // let mut prep_sums = Vec::new();
            let mut input_slots = Vec::with_capacity(node.inputs().len());

            // Process every single input socket to calculate fan-in mapping metadata
            for sources in raw_input_sources {
                match sources.len() {
                    0 => {
                        // Unconnected: Route straight to the safe permanent Silence slot
                        input_slots.push(0);
                    }
                    1 => {
                        // Normal 1-to-1 link: Bind node straight to the source buffer slot
                        input_slots.push(sources[0]);
                    }
                    _ => {
                        panic!("multiple nodes in one socket!")
                    }
                }
            }

            let output_slots: Vec<SlotIndex> = (0..node.outputs().len())
                .map(|i| output_slot[&(node_id, i as SocketIndex)])
                .collect();

            steps.push(ScheduleStep {
                node_id,

                input_slots,
                output_slots,
            });
        }

        let master_output_slot = *output_slot
            .get(&(self.master_node_id, 0))
            .ok_or(EngineError::NodeNotFound(self.master_node_id))?;

        Ok(CompiledGraph {
            steps,
            buffer_count, // Reflects the combination of outputs + required scratch spaces
            master_output_slot,
        })
    }
}

impl Default for ProjectData {
    fn default() -> Self {
        Self::new()
    }
}
