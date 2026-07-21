use crate::{
    engine::{SlotIndex, bbp::PoolExecutor, engineconfig::EngineConfig, tick::Tick},
    model::{
        DataKind,
        flow::{Node, Param, Socket},
        project::ProjectData,
    },
};
use std::any::Any;

#[derive(Debug, Clone)]
pub struct LowpassFilter {
    pub cutoff_hz: Param, // live-tweakable from any thread, wait-free
}

impl LowpassFilter {
    const INPUTS: &'static [Socket] = &[Socket {
        kind: DataKind::Audio,
        name: "in",
        visible: true,
    }];
    const OUTPUTS: &'static [Socket] = &[Socket {
        kind: DataKind::Audio,
        name: "out",
        visible: true,
    }];
    pub const AUDIO_IN: SlotIndex = 0;

    pub fn new(cutoff_hz: f32) -> Self {
        Self {
            cutoff_hz: Param::new(cutoff_hz),
        }
    }
}

/// Runtime-only state: one previous output sample per channel.
/// Exclusively owned by the audio thread, never cloned, never shared.
struct LowpassState {
    prev: Vec<f32>,
}

impl Node for LowpassFilter {
    fn inputs(&self) -> &[Socket] {
        Self::INPUTS
    }
    fn outputs(&self) -> &[Socket] {
        Self::OUTPUTS
    }

    fn init_state(&self, channels: u16) -> Box<dyn Any + Send> {
        Box::new(LowpassState {
            prev: vec![0.0; channels.into()],
        })
    }

    fn process(
        &self,
        _project: &ProjectData,
        pool: &mut PoolExecutor,
        state: &mut dyn Any,
        _block_start: Tick,
        config: &EngineConfig,
        inputs: &[SlotIndex],
        outputs: &[SlotIndex],
    ) {
        let sample_rate = config.config.sample_rate as f32;
        let state = state
            .downcast_mut::<LowpassState>()
            .expect("state pool handed LowpassFilter the wrong state type");

        let channels = config.config.channels as usize;
        // Vec grows only on the very first block after a topology change
        // that added this node — and reconciliation already sized it via
        // init_state for the common case, so in steady state this is a no-op.
        if state.prev.len() != channels {
            panic!("Invalid config")
        }

        let cutoff = self.cutoff_hz.get().max(1.0); // avoid div-by-zero at 0 Hz
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
        let alpha = (1.0 / sample_rate) / (rc + 1.0 / sample_rate);

        let input_buf = pool.get_input(inputs[0]);
        let output_buf = pool.get_output(outputs[0]);

        for (frame, chunk) in input_buf.chunks(channels).enumerate() {
            let out_start = frame * channels;
            for (ch, &x) in chunk.iter().enumerate() {
                state.prev[ch] += alpha * (x - state.prev[ch]);
                output_buf[out_start + ch] = state.prev[ch];
            }
        }
    }
}
