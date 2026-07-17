use crate::model::{
    DataKind,
    arr::{
        clip::{Clip, ClipID},
        track::{Track, TrackID},
    },
    asset::{Asset, AssetID},
    flow::{NativeNodeType, Node, NodeGraph, NodeID, NodePayload, Socket},
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
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}
