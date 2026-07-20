//! Module for Flow related types
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::model::{Audio, Kind, arr::track::AudioTrackID};

new_key_type! {
    pub struct NodeID;
    pub struct LinkID;
}

pub type SocketIndex = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Socket<K: Kind> {
    pub kind: PhantomData<K>,
    pub name: String,
    pub visible: bool,
}

impl<K: Kind> Socket<K> {
    pub fn new<C: Kind>(name: impl Into<String>, visible: bool) -> Socket<C> {
        Socket::<C> {
            kind: PhantomData,
            name: name.into(),
            visible,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub inputs: Vec<Socket<Audio>>,
    pub outputs: Vec<Socket<Audio>>,
    pub payload: NodePayload,
}

impl Node {
    pub fn new(
        inputs: impl IntoIterator<Item = Socket<Audio>>,
        outputs: impl IntoIterator<Item = Socket<Audio>>,
        payload: NodePayload,
    ) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            outputs: outputs.into_iter().collect(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodePayload {
    Native(NativeNodeType),
    AudioTrackReader(AudioTrackID),
    Group(Box<NodeGraph>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeNodeType {
    Master,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub from: (NodeID, SocketIndex),
    pub to: (NodeID, SocketIndex),
}

/// A graph representing the signal flow between nodes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeGraph {
    pub nodes: SlotMap<NodeID, Node>,
    pub links: SlotMap<LinkID, Link>,
}
