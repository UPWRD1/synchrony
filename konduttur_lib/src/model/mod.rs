use std::any::Any;

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
pub trait DataKind: Send + Sync + PartialEq + 'static {
    fn can_connect_to<U: 'static>(&self, dest: U) -> bool {
        self.type_id() == dest.type_id()
            || (self.type_id() == AudioKind.type_id() && dest.type_id() == CvKind.type_id())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioKind;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiKind;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CvKind;

impl DataKind for AudioKind {}
impl DataKind for MidiKind {}
impl DataKind for CvKind {}

pub trait Renderable {
    fn render(&self, proj: &ProjectData, audio_buf: &mut [f32], block_start: Tick, channels: u16);
}
