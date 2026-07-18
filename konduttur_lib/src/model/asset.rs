use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

use crate::{
    engine::tick::Tick,
    model::{AudioKind, TypedKey},
};

new_key_type! {
    pub struct AssetID;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAsset {
    #[serde(skip)]
    pub samples: Arc<Vec<f32>>,
    pub gain: f32,
    pub channels: u16,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineMidiEvent {
    pub absolute_tick: Tick,
    pub bytes: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiSequence {
    pub events: Vec<TimelineMidiEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub absolute_tick: Tick,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationCurve {
    pub points: Vec<AutomationPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRegistry {
    pub audio: SlotMap<AssetID, AudioAsset>,
    pub midi: SlotMap<AssetID, MidiSequence>,
    pub cv: SlotMap<AssetID, AutomationCurve>,
}
