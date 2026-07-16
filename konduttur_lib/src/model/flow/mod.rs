//! Module for Flow related types
use slotmap::{SlotMap, new_key_type};

use crate::model::{DataKind, arr::track::TrackID};

new_key_type! {
    pub struct NodeID;
    pub struct LinkID;
}

pub type SocketIndex = u16;

#[derive(Debug, Clone)]
pub struct Socket {
    pub kind: DataKind,
    pub name: String,
}

impl Socket {
    pub fn new(kind: DataKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub inputs: Vec<Socket>,
    pub outputs: Vec<Socket>,
    pub payload: NodePayload,
}

impl Node {
    pub fn new(
        inputs: impl IntoIterator<Item = Socket>,
        outputs: impl IntoIterator<Item = Socket>,
        payload: NodePayload,
    ) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            outputs: outputs.into_iter().collect(),
            payload,
        }
    }
}

#[derive(Debug, Clone)]
pub enum NodePayload {
    Native(NativeNodeType),
    TrackReader(TrackID),
    Group(Box<NodeGraph>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeNodeType {
    Master,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub from: (NodeID, SocketIndex),
    pub to: (NodeID, SocketIndex),
}

/// A graph representing the signal flow between nodes.
#[derive(Debug, Clone, Default)]
pub struct NodeGraph {
    pub nodes: SlotMap<NodeID, Node>,
    pub links: SlotMap<LinkID, Link>,
}
