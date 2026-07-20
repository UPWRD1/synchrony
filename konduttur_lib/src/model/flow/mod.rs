//! Module for Flow related types
use std::marker::PhantomData;

use dyn_clone::DynClone;
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::{
    engine::{PoolExecutor, SlotIndex, tick::Tick},
    model::{Audio, DataKind, Kind, Renderable, Stored, project::ProjectData},
};

new_key_type! {
    pub struct NodeID;
    pub struct LinkID;
}

pub type SocketIndex = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Socket {
    pub kind: DataKind,
    pub name: &'static str,
    pub visible: bool,
}

impl Socket {
    pub fn new<C: Kind>(name: impl Into<String>, visible: bool) -> Socket {
        Self {
            kind: C::into_datakind(),
            name: name.into().leak(),
            visible,
        }
    }
}

pub trait Node: std::fmt::Debug + DynClone + Send + Sync + 'static {
    fn inputs<'a>(&'a self) -> &'a [Socket];
    fn outputs(&self) -> &[Socket];

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
    input: Socket,
}

impl Master {
    const INPUTS: &'static [Socket] = &[Socket {
        kind: DataKind::Audio,
        name: "input",
        visible: true,
    }];

    const OUTPUTS: &'static [Socket] = &[];
    pub fn new() -> Self {
        Self {
            input: Socket::new::<Audio>("input", true),
        }
    }
}

impl Node for Master {
    fn inputs(&self) -> &'static [Socket] {
        Self::INPUTS
    }

    fn outputs(&self) -> &'static [Socket] {
        Self::OUTPUTS
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
    output: Socket,
    id: <K::Track as Stored>::Id,
}

impl<K: Kind> TrackReader<K> {
    pub fn new(id: <K::Track as Stored>::Id) -> Self {
        Self {
            kind: PhantomData,
            output: Socket::new::<K>("audio out", true),
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

    fn inputs<'a>(&'a self) -> &'a [Socket] {
        todo!()
    }

    fn outputs(&self) -> &[Socket] {
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
