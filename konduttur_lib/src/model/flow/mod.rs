//! Module for Flow related types
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use dyn_clone::DynClone;
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::{
    engine::{SlotIndex, bbp::PoolExecutor, tick::Tick},
    model::{DataKind, Kind, project::ProjectData},
};

pub mod nodes;

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

#[derive(Debug, Clone)]
pub struct Param(Arc<AtomicU32>); // f32 via to_bits/from_bits

impl Param {
    pub fn new(v: f32) -> Self {
        Self(Arc::new(AtomicU32::new(v.to_bits())))
    }
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

pub trait Node: std::fmt::Debug + DynClone + Send + Sync + 'static {
    fn inputs(&self) -> &[Socket];
    fn outputs(&self) -> &[Socket];

    fn process(
        &self,
        project: &ProjectData,
        pool: &mut PoolExecutor,
        block_start: Tick,
        channels: u16,
        inputs: &[SlotIndex],
        outputs: &[SlotIndex],
    );
}

dyn_clone::clone_trait_object!(Node);

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
