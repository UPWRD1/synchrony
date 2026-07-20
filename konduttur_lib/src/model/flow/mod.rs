//! Module for Flow related types
use std::marker::PhantomData;

use dyn_clone::DynClone;
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::{
    engine::{PoolExecutor, SlotIndex, tick::Tick},
    model::{Audio, Kind, Renderable, Stored, project::ProjectData},
};

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
#[derive(Debug, Clone)]
pub enum SocketKind {
    Audio(Socket<Audio>),
}

impl From<Socket<Audio>> for SocketKind {
    fn from(value: Socket<Audio>) -> Self {
        SocketKind::Audio(value)
    }
}

pub trait Node: std::fmt::Debug + DynClone + Send + Sync + 'static {
    fn inputs(&self) -> &Vec<SocketKind>;
    fn outputs(&self) -> &Vec<SocketKind>;

    fn process(
        &self,
        project: &ProjectData,
        pool: &mut PoolExecutor,
        block_start: Tick,
        channels: u16,
        inputs: &Vec<SlotIndex>,
        outputs: &Vec<SlotIndex>,
    );
}

dyn_clone::clone_trait_object!(Node);

#[derive(Debug, Clone)]
pub struct Master {
    input: Socket<Audio>,
}

impl Master {
    pub fn new() -> Self {
        Self {
            input: Socket::<Audio>::new("input", true),
        }
    }
}

impl Node for Master {
    fn inputs(&self) -> &Vec<SocketKind> {
        todo!()
    }

    fn outputs(&self) -> &Vec<SocketKind> {
        todo!()
    }
    fn process(
        &self,
        project: &ProjectData,
        pool: &mut PoolExecutor,
        block_start: Tick,
        channels: u16,
        inputs: &Vec<SlotIndex>,
        outputs: &Vec<SlotIndex>,
    ) {
        let input_buf = pool.get_input(inputs[0]);
        let output_buf = pool.get_output(outputs[0]);

        output_buf.copy_from_slice(input_buf);
    }
}

#[derive(Debug, Clone)]
pub struct TrackReader<K: Kind> {
    kind: PhantomData<K>,
    output: Socket<K>,
    id: <K::Track as Stored>::Id,
}

impl<K: Kind> TrackReader<K> {
    pub fn new(id: <K::Track as Stored>::Id) -> Self {
        Self {
            kind: PhantomData,
            output: Socket::<K>::new("audio out", true),
            id,
        }
    }
}

impl Node for TrackReader<Audio> {
    fn process(
        &self,
        project: &ProjectData,
        pool: &mut PoolExecutor,
        block_start: Tick,
        channels: u16,
        inputs: &Vec<SlotIndex>,
        outputs: &Vec<SlotIndex>,
    ) {
        if let Some(track) = project.tracks.get(self.id) {
            let output_buf = pool.get_output(outputs[0]);
            track.render(project, output_buf, block_start, channels);
        }
    }

    fn inputs(&self) -> &Vec<SocketKind> {
        todo!()
    }

    fn outputs(&self) -> &Vec<SocketKind> {
        todo!()
    }
}

struct Group {}

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
#[derive(Debug, Default, Clone)]
pub struct NodeGraph {
    pub nodes: SlotMap<NodeID, Box<dyn Node>>,
    pub links: SlotMap<LinkID, Link>,
}
