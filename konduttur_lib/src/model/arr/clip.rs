use serde::{Deserialize, Serialize, de::DeserializeOwned};
use slotmap::new_key_type;

use crate::{
    engine::{engineconfig::EngineConfig, tick::Tick},
    model::{
        Audio, Kind, Renderable, Stored,
        asset::{AssetState, AudioAssetID},
    },
};

new_key_type! {
    pub struct AudioClipID;
}

pub trait Clip<K: Kind>: Sized + Serialize + DeserializeOwned {
    fn new(start: Tick, length: Tick, asset_id: <K::Asset as Stored>::Id) -> Self;

    fn start_mut(&mut self) -> &mut Tick;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioClip {
    pub start: Tick,
    pub length: Tick,
    pub asset_id: AudioAssetID,
}

impl Stored for AudioClip {
    type Id = AudioClipID;

    fn access(project: &crate::model::project::ProjectData) -> &slotmap::SlotMap<Self::Id, Self> {
        &project.clips
    }

    fn access_mut(
        project: &mut crate::model::project::ProjectData,
    ) -> &mut slotmap::SlotMap<Self::Id, Self> {
        &mut project.clips
    }
}

impl Clip<Audio> for AudioClip {
    fn new(start: Tick, length: Tick, asset_id: <<Audio as Kind>::Asset as Stored>::Id) -> Self {
        Self {
            start,
            length,
            asset_id,
        }
    }

    fn start_mut(&mut self) -> &mut Tick {
        &mut self.start
    }
}

impl Renderable for AudioClip {
    fn render(
        &self,
        proj: &crate::model::project::ProjectData,
        buf: &mut [f32],
        block_start: Tick,
        config: &EngineConfig,
    ) {
        let Some(asset) = proj.assets.get(self.asset_id) else {
            return;
        };
        let AssetState::Ready(data) = &asset.state else {
            return;
        };

        let channels = config.config.channels;

        let block_len: Tick = (buf.len() / channels as usize).into();
        let block_end = block_start + block_len;

        let clip_end = self.start + self.length;
        let overlap_start = block_start.max(self.start);
        let overlap_end = block_end.min(clip_end);
        if overlap_start >= overlap_end {
            panic!("eventually figure out what goes here");
        }
        for frame in (overlap_start.0)..overlap_end.0 {
            let src_idx = ((frame - self.start.0) as usize) * data.channels as usize;
            let dst_idx = ((frame - block_start.0) as usize) * channels as usize;
            for ch in 0..channels as usize {
                let src_ch = ch.min(data.channels as usize - 1);
                if let (Some(&sample), Some(dest)) = (
                    data.samples.get(src_idx + src_ch),
                    buf.get_mut(dst_idx + ch),
                ) {
                    *dest += sample * data.gain;
                }
            }
        }
    }
}
