use std::{collections::BTreeMap, sync::OnceLock};

use slotmap::new_key_type;

use crate::{
    engine::tick::Tick,
    model::{
        DataKind,
        arr::clip::{ClipData, ClipID},
        flow::NodeID,
        project::Project,
    },
};

new_key_type! {
   pub struct TrackID;
}

#[derive(Debug, Clone)]
pub struct Track {
    pub name: String,
    pub clips: BTreeMap<Tick, ClipID>,
    pub gain: f32,
    pub kind: DataKind,
    pub linked_node_id: OnceLock<NodeID>,
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
            for frame in (overlap_start.0)..overlap_end.0 {
                let src_idx = ((frame - clip.start.0) as usize) * asset.channels as usize;
                let dst_idx = ((frame - block_start.0) as usize) * channels as usize;
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
