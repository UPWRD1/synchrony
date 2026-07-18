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
    use crate::engine::{AddClip, AddTrack};
    use crate::model::project::ProjectData;
    use anyhow::Result;

    use std::sync::Arc;

    use engine::Engine;
    use model::DataKind;
    #[test]
    fn it_works() {
        helper();
    }

    fn helper() -> Result<()> {
        let project = Arc::new(ProjectData::new());

        // --- 2. Build the project through the real API, not by hand --------
        let mut engine = Engine::new(project)?;

        let clap_asset = engine.load_asset(assetserver::load_audio_asset("./assets/reliable.wav")?);
        let snap_asset = engine.load_asset(assetserver::load_audio_asset("./assets/snare.wav")?);

        let clap_len = {
            let asset = &engine.project().assets.audio[clap_asset];
            asset.samples.len() as u64 / asset.channels as u64
        } * 2;
        let snap_len = {
            let asset = &engine.project().assets.audio[snap_asset];
            asset.samples.len() as u64 / asset.channels as u64
        } * 2;
        let mut inc_clap_start = 0;
        let mut inc_snap_start = clap_len;
        for _ in 0..12 {
            let clap_track = engine.apply(AddTrack {
                name: format!("Snap_{inc_snap_start}",),
                kind: DataKind::Audio,
            })?;
            let snap_track = engine.apply(AddTrack {
                name: format!("Clap_{inc_clap_start}"),
                kind: DataKind::Audio,
            })?;

            engine.apply(AddClip {
                track: clap_track,
                start: engine::tick::Tick(inc_clap_start),
                end: engine::tick::Tick(clap_len),
                asset: clap_asset,
            })?;
            engine.apply(AddClip {
                track: snap_track,
                start: engine::tick::Tick(inc_snap_start),
                end: engine::tick::Tick(snap_len),
                asset: snap_asset,
            })?;
            inc_clap_start += clap_len + snap_len;
            inc_snap_start += snap_len;
        }

        engine.play()?;

        Ok(())
    }
}
