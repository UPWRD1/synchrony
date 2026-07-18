use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::{
    engine::tick::Tick,
    model::{
        AudioKind, CvKind, DataKind, MidiKind, Renderable, TypedKey, arr::clip::ClipID,
        asset::AssetRegistry, flow::NodeID, project::ProjectData,
    },
};

new_key_type! {
   pub struct TrackID;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track<D: DataKind> {
    pub name: String,
    pub clips: BTreeMap<Tick, TypedKey<ClipID, D>>,
    pub gain: f32,
    pub linked_node_id: Option<NodeID>,
}

// Convenient domain aliases
pub type AudioTrack = Track<AudioKind>;
pub type MidiTrack = Track<MidiKind>;
pub type CvTrack = Track<CvKind>;

impl Renderable for Track<AudioKind> {
    fn render(&self, proj: &ProjectData, buf: &mut [f32], block_start: Tick, channels: u16) {
        // Deinterleave
        let block_len: Tick = (buf.len() / channels as usize).into();
        let block_end = block_start + block_len;

        let lookback = self
            .clips
            .range(..block_start)
            .next_back()
            .map(|(_, id)| *id)
            .filter(|id| {
                proj.clips
                    .get(*id)
                    .is_some_and(|c| c.start + c.length > block_start)
            });
        let active = lookback
            .into_iter()
            .chain(self.clips.range(block_start..block_end).map(|(_, id)| *id));

        for clip_id in active {
            let Some(clip) = proj.clips.get(clip_id) else {
                panic!("Invalid clip");
            };
            clip.render(proj, buf, block_start, channels)
        }
    }
}

impl Renderable for Track<CvKind> {
    fn render(&self, proj: &ProjectData, buf: &mut [f32], block_start: Tick, channels: u16) {
        todo!();
    }
}
