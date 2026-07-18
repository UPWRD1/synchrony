pub mod engine;
pub mod model;

use assert_no_alloc::*;

#[cfg(debug_assertions)] // required when disable_release is set (default)
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::assetserver;
    use crate::model::project::ProjectData;
    use anyhow::Result;

    use std::sync::Arc;

    use engine::{Command, Engine};
    use model::DataKind;
    #[test]
    fn it_works() {
        helper();
    }

    fn helper() -> Result<()> {
        let project = Arc::new(ProjectData::new());

        // --- 2. Build the project through the real API, not by hand --------
        let mut engine = Engine::new(project)?;

        let clap_asset = engine.load_asset(assetserver::load_audio_asset(
            "./assets/clap.mp3",
            engine.config.config.sample_rate,
        )?);
        let snap_asset = engine.load_asset(assetserver::load_audio_asset(
            "./assets/snap.mp3",
            engine.config.config.sample_rate,
        )?);

        let clap_len = {
            let asset = &engine.project().assets[clap_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };
        let snap_len = {
            let asset = &engine.project().assets[snap_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };
        let mut inc_clap_start = 0;
        let mut inc_snap_start = clap_len;
        for _ in 0..12 {
            engine.apply(Command::AddTrack {
                name: "Snap".into(),
                kind: DataKind::Audio,
            })?;
            engine.apply(Command::AddTrack {
                name: "Clap".into(),
                kind: DataKind::Audio,
            })?;

            let (clap_track, snap_track) = {
                let ids: Vec<_> = engine.project().tracks.keys().collect();
                (ids[0], ids[1])
            };

            engine.apply(Command::AddClip {
                track: clap_track,
                start: engine::tick::Tick(inc_clap_start),
                end: engine::tick::Tick(clap_len),
                asset: clap_asset,
            })?;
            engine.apply(Command::AddClip {
                track: snap_track,
                start: engine::tick::Tick(inc_snap_start),
                end: engine::tick::Tick(snap_len),
                asset: snap_asset,
            })?;
            inc_clap_start += clap_len + snap_len;
            inc_snap_start += clap_len + snap_len;
        }

        engine.apply(Command::Play)?;

        Ok(())
    }
}
