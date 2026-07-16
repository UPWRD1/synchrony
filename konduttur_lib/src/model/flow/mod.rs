//! Module for Flow related types
use std::collections::HashMap;

use slotmap::{SlotMap, new_key_type};

use crate::model::arr::track::TrackID;

new_key_type! {
    pub struct NodeID;
    pub struct LinkID;
}

pub type SocketIndex = u16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SocketKind {
    Audio,
    Midi,
    Cv,
}

impl SocketKind {
    pub fn can_connect_to(self, dest: Self) -> bool {
        self == dest || (self == Self::Audio && dest == Self::Cv)
    }
}

#[derive(Clone)]
pub struct Socket {
    kind: SocketKind,
}

#[derive(Clone)]
pub struct Node {
    pub inputs: HashMap<String, Socket>,
    pub outputs: HashMap<String, Socket>,
    pub payload: NodePayload,
}

#[derive(Clone)]
pub enum NodePayload {
    Native(NativeNodeType),
    TrackReader(TrackID),
    Group(Box<NodeGraph>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeNodeType {
    Master,
}

#[derive(Clone)]
pub struct Link {
    pub from: (NodeID, SocketIndex),
    pub to: (NodeID, SocketIndex),
}

/// A graph representing the signal flow between nodes.
#[derive(Clone, Default)]
pub struct NodeGraph {
    pub nodes: SlotMap<NodeID, Node>,
    pub links: SlotMap<LinkID, Link>,
}
