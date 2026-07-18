use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiMessage {
    /// Sample offset *within the current audio block* [0..block_size)
    pub frame_offset: u32,
    /// Standard 3-byte MIDI payload (Status, Data1, Data2)
    pub bytes: [u8; 3],
}

pub struct MidiBufferSlot {
    /// Bounded storage to prevent real-time dynamic vector allocations
    pub events: [MidiMessage; 32],
    pub count: usize,
}

impl MidiBufferSlot {
    #[inline]
    pub fn clear(&mut self) {
        self.count = 0;
    }
    #[inline]
    pub fn as_slice(&self) -> &[MidiMessage] {
        &self.events[..self.count]
    }

    #[inline]
    pub fn push(&mut self, msg: MidiMessage) {
        if self.count < 32 {
            self.events[self.count] = msg;
            self.count += 1;
        }
    }
}
