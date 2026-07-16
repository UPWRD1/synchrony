use std::collections::BTreeMap;

use slotmap::new_key_type;

use crate::{
    engine::Tick,
    model::{
        arr::clip::{ClipData, ClipID},
        project::Project,
    },
};

new_key_type! {
   pub struct TrackID;
}

pub struct Track {
    pub name: String,
    pub id: u64,
    pub clips: BTreeMap<Tick, ClipID>,
    pub gain: f32,
}

impl Track {
    pub fn render_into_buf(
        &self,
        proj: &Project,
        buf: &mut [f32],
        block_start: Tick,
        channels: u16,
    ) {
        // Deinterleave
        let block_len = (buf.len() / channels as usize) as Tick;
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
            let ClipData::Audio(audio) = &clip.data else {
                panic!("Non-audio clip")
            };
            let Some(asset) = proj.assets.get(*audio) else {
                panic!("invalid asset");
            };

            let clip_end = clip.start + clip.length;
            let overlap_start = block_start.max(clip.start);
            let overlap_end = block_end.min(clip_end);
            if overlap_start >= overlap_end {
                continue;
            }
            for frame in overlap_start..overlap_end {
                let src_idx = ((frame - clip.start) as usize) * asset.channels as usize;
                let dst_idx = ((frame - block_start) as usize) * channels as usize;
                for ch in 0..channels as usize {
                    let src_ch = ch.min(asset.channels as usize - 1);
                    if let (Some(&sample), Some(dest)) = (
                        asset.samples.get(src_idx + src_ch),
                        buf.get_mut(dst_idx + ch),
                    ) {
                        *dest += sample * asset.gain * self.gain;
                    }
                }
            }
        }
    }
}
