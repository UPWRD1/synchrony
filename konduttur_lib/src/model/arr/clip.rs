use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::{
    engine::tick::Tick,
    model::{DataKind, Renderable, asset::AssetID},
};

new_key_type! {
    pub struct ClipID;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Clip<D: DataKind> {
    pub start: Tick,
    pub length: Tick,
    pub data: AssetID<D>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ClipData {
    Audio(AssetID),
    Midi,
    CV,
}

impl Renderable for Clip {
    fn render(
        &self,
        proj: &crate::model::project::ProjectData,
        audio_buf: &mut [f32],
        block_start: Tick,
        channels: u16,
    ) {
        match self.data {
            ClipData::Audio(asset_id) => {
                let block_len: Tick = (audio_buf.len() / channels as usize).into();
                let block_end = block_start + block_len;
                let Some(asset) = proj.assets.get(asset_id) else {
                    panic!("clip's asset does not exist");
                };

                let clip_end = self.start + self.length;
                let overlap_start = block_start.max(self.start);
                let overlap_end = block_end.min(clip_end);
                if overlap_start >= overlap_end {
                    panic!("eventually figure out what goes here");
                }
                for frame in (overlap_start.0)..overlap_end.0 {
                    let src_idx = ((frame - self.start.0) as usize) * asset.channels as usize;
                    let dst_idx = ((frame - block_start.0) as usize) * channels as usize;
                    for ch in 0..channels as usize {
                        let src_ch = ch.min(asset.channels as usize - 1);
                        if let (Some(&sample), Some(dest)) = (
                            asset.samples.get(src_idx + src_ch),
                            audio_buf.get_mut(dst_idx + ch),
                        ) {
                            *dest += sample * asset.gain;
                        }
                    }
                }
            }
            ClipData::Midi => todo!(),
            ClipData::CV => todo!(),
        }
    }
}
