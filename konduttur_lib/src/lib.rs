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
    use crate::model::Audio;
    use crate::model::project::ProjectData;
    use anyhow::Result;

    use std::sync::Arc;

    use engine::Engine;
    #[test]
    fn it_works() {
        helper();
    }

    fn helper() -> Result<()> {
        let project = Arc::new(ProjectData::new());

        // --- 2. Build the project through the real API, not by hand --------
        let mut engine = Engine::new(project)?;

        let clap_asset = engine.load_asset(assetserver::load_audio_asset("./assets/clap.mp3")?);
        let snap_asset = engine.load_asset(assetserver::load_audio_asset("./assets/snap.mp3")?);

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
            let clap_track = engine.apply(AddTrack {
                name: format!("Snap_{inc_snap_start}",),
                kind: Audio,
            })?;
            let snap_track = engine.apply(AddTrack {
                name: format!("Clap_{inc_clap_start}"),
                kind: Audio,
            })?;

            engine.apply(AddClip::<Audio> {
                track: clap_track,
                start: engine::tick::Tick(inc_clap_start),
                end: engine::tick::Tick(clap_len),
                asset_id: clap_asset,
            })?;
            engine.apply(AddClip::<Audio> {
                track: snap_track,
                start: engine::tick::Tick(inc_snap_start),
                end: engine::tick::Tick(snap_len),
                asset_id: snap_asset,
            })?;
            inc_clap_start += clap_len + snap_len;
            inc_snap_start += clap_len + snap_len;
        }

        engine.transport.play();
        println!("Playing... press enter to quit");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        engine.transport.stop();
        Ok(())
    }
}
