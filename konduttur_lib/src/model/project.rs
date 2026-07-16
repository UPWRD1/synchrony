use crate::model::{
    arr::{
        clip::{Clip, ClipID},
        track::{Track, TrackID},
    },
    asset::{Asset, AssetID},
};

use slotmap::SlotMap;
pub struct Project {
    pub tracks: SlotMap<TrackID, Track>,
    pub clips: SlotMap<ClipID, Clip>,
    pub assets: SlotMap<AssetID, Asset>,
}
