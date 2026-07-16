use slotmap::new_key_type;

use crate::{engine::Tick, model::asset::AssetID};

new_key_type! {
    pub struct ClipID;
}
pub struct Clip {
    pub start: Tick,
    pub length: Tick,
    pub data: ClipData,
}

pub enum ClipData {
    Audio(AssetID),
    Midi,
    CV,
}
