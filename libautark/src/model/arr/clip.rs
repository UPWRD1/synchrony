use crate::{
    engine::{ActorRef, asset::AssetActor, commands::WaitForAudioAsset, tick::Tick},
    model::{
        Audio, Kind, RenderBlock, Renderable, Stored,
        asset::{AudioAsset, AudioAssetID, AudioAssetPayload},
        project::ProjectData,
    },
};
use anyhow::Result;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use slotmap::new_key_type;

new_key_type! {
    pub struct AudioClipID;
}

pub trait Clip<K: Kind>: Sized + Serialize + DeserializeOwned {
    fn new(start: Tick, length: Tick, asset_id: <K::Asset as Stored>::ID) -> Self;

    fn start_mut(&mut self) -> &mut Tick;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AudioClip {
    pub start: Tick,
    pub length: Tick,
    pub asset_id: AudioAssetID,
}

impl Stored for AudioClip {
    type ID = AudioClipID;
    type Location = ProjectData;
    type Storage = Self;

    fn access(project: &ProjectData) -> &slotmap::SlotMap<Self::ID, Self> {
        &project.clips
    }

    fn access_mut(project: &mut ProjectData) -> &mut slotmap::SlotMap<Self::ID, Self> {
        &mut project.clips
    }
}

impl Clip<Audio> for AudioClip {
    fn new(start: Tick, length: Tick, asset_id: <<Audio as Kind>::Asset as Stored>::ID) -> Self {
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

#[derive(Debug, Clone)]
pub struct ResolvedAudioClip {
    pub start_frame: Tick,
    pub length_frames: Tick,
    asset: AudioAsset,
}

impl ResolvedAudioClip {
    pub async fn from_clip(clip: AudioClip, asset_h: ActorRef<AssetActor>) -> Result<Self> {
        let asset = asset_h.call(WaitForAudioAsset(clip.asset_id)).await?;

        Ok(Self {
            start_frame: clip.start,
            length_frames: clip.length,
            asset: asset.clone(),
        })
    }
}

impl Renderable for ResolvedAudioClip {
    fn render(
        &self,
        RenderBlock {
            buf,
            block_start,
            block_end,
            channels,
        }: &mut RenderBlock,
    ) {
        match &self.asset.payload {
            AudioAssetPayload::ResidentInterleaved(samples) => {
                let clip_end = self.start_frame + self.length_frames;
                let overlap_start = (*block_start).max(self.start_frame);
                let overlap_end = (*block_end).min(clip_end);
                assert!(
                    overlap_start < overlap_end,
                    "eventually figure out what goes here"
                );
                for frame in (overlap_start.0)..overlap_end.0 {
                    let src_idx =
                        ((frame - self.start_frame.0) as usize) * self.asset.channels as usize;
                    let dst_idx = ((frame - block_start.0) as usize) * *channels as usize;
                    for dest_ch in 0..*channels as usize {
                        let src_ch = dest_ch.min(self.asset.channels as usize - 1);
                        let (sample, dest) =
                            (samples[src_idx + src_ch], &mut buf[dst_idx + dest_ch]);
                        *dest += sample * self.asset.gain;
                    }
                }
            }
            AudioAssetPayload::StreamingInterleaved => todo!(),
        }
    }
}
