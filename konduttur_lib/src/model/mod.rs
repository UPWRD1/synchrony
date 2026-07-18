use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::{engine::tick::Tick, model::project::ProjectData};

pub mod arr;
pub mod asset;
pub mod flow;
pub mod project;

// #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// pub enum DataKind {
//     Audio,
//     Midi,
//     Cv,
// }

// The root constraint trait
pub trait DataKind: Send + Sync + PartialEq {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioKind;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiKind;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CvKind;

impl DataKind for AudioKind {}
impl DataKind for MidiKind {}
impl DataKind for CvKind {}

// K is the raw slotmap key type, T is our DataType marker
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct TypedKey<K: slotmap::Key, D: DataKind> {
    pub raw: K,
    _marker: PhantomData<D>,
}

impl<K: slotmap::Key, D: DataKind> TypedKey<K, D> {
    pub fn new(raw: K) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }
}
pub trait Renderable {
    fn render(&self, proj: &ProjectData, audio_buf: &mut [f32], block_start: Tick, channels: u16);
}
