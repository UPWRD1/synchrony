use slotmap::new_key_type;

use crate::{engine::tick::Tick, model::asset::AssetID};

new_key_type! {
    pub struct ClipID;
}

#[derive(Debug, Clone, Copy)]
pub struct Clip {
    pub start: Tick,
    pub length: Tick,
    pub data: ClipData,
}

#[derive(Debug, Clone, Copy)]
pub enum ClipData {
    Audio(AssetID),
    Midi,
    CV,
}
