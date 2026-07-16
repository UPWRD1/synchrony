pub mod engine;
pub mod model;

use assert_no_alloc::*;

#[cfg(debug_assertions)] // required when disable_release is set (default)
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineError, assetserver};
    use crate::model::project::Project;
    use anyhow::{Context, Result};

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use arc_swap::ArcSwap;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};

    use engine::{BufferPool, Command, Engine, RenderState, execute_block};
    use model::DataKind;
    #[test]
    fn it_works() {
        helper();
    }

    fn helper() -> Result<()> {
        // --- 1. Host -> Device -> Config -----------------------------------
        let host = cpal::default_host();
        let device = host.default_output_device().context("no output device")?;
        let supported = device.default_output_config()?;
        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let config: StreamConfig = supported.into();

        println!("output device config: {sample_rate} Hz, {channels} ch, format {sample_format:?}");

        let project = Arc::new(Project::new());
        dbg!(&project);
        // --- 2. Build the project through the real API, not by hand --------
        let mut engine = Engine::new(project);

        let kick_asset = engine.load_asset(assetserver::load_audio_asset(
            "./../assets/clap.m4a",
            sample_rate,
        )?);
        let bass_asset = engine.load_asset(assetserver::load_audio_asset(
            "./../assets/snap.m4a",
            sample_rate,
        )?);

        engine.apply(Command::AddTrack {
            name: "Snap".into(),
            kind: DataKind::Audio,
        })?;
        engine.apply(Command::AddTrack {
            name: "Clap".into(),
            kind: DataKind::Audio,
        })?;

        let (kick_track, bass_track) = {
            let ids: Vec<_> = engine.project().tracks.keys().collect();
            (ids[0], ids[1])
        };

        let kick_len = {
            let asset = &engine.project().assets[kick_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };
        let bass_len = {
            let asset = &engine.project().assets[bass_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };

        engine.apply(Command::AddClip {
            track: kick_track,
            start: engine::Tick(0),
            length: engine::Tick(kick_len),
            asset: kick_asset,
        })?;
        // Starts right where the kick clip ends -- sequenced across two tracks.
        engine.apply(Command::AddClip {
            track: bass_track,
            start: engine::Tick(kick_len),
            length: engine::Tick(bass_len),
            asset: bass_asset,
        })?;

        // --- 3. Hand the audio thread its read handle -----------------------
        let render_state: Arc<ArcSwap<RenderState>> = engine.render_state_handle();
        let playhead = Arc::new(AtomicU64::new(0));

        let stream = match sample_format {
            SampleFormat::F32 => {
                build_stream::<f32>(&device, &config, channels, render_state, playhead)?
            }
            other => anyhow::bail!(
                "device wants sample format {other:?}; only f32 output is wired up in this skeleton \
             (TODO: convert via cpal::Sample for I16/U16 devices)"
            ),
        };

        // Stream has to stay alive for audio to keep playing -- this local
        // binding, held until the end of main(), is what does that.
        stream.play()?;

        println!("playing... press enter to quit");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(())
    }

    /// Generic over the sample type cpal actually wants; only instantiated
    /// for f32 today (see the SampleFormat match above), but written this way
    /// so adding I16/U16 conversion later is a second match arm, not a rewrite.
    fn build_stream<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        render_state: Arc<ArcSwap<RenderState>>,
        playhead: Arc<AtomicU64>,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let mut pool = BufferPool::new();

        let stream = device.build_output_stream(
            *config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                let frame_count = data.len() / channels as usize;
                let start = playhead.fetch_add(frame_count as u64, Ordering::Relaxed);

                // The entire real-time path: load the current published state
                // (lock-free), run the compiled schedule, copy the result out
                // converting f32 -> whatever cpal wants.
                let state = render_state.load();
                let mixed = execute_block(
                    &state.schedule,
                    &state.project,
                    engine::Tick(start),
                    frame_count,
                    channels,
                    &mut pool,
                );

                for (dst, &src) in data.iter_mut().zip(mixed) {
                    *dst = T::from_sample(src);
                }
            },
            move |err| eprintln!("audio stream error: {err}"),
            None, // no timeout on stream creation
        )?;

        Ok(stream)
    }
}
