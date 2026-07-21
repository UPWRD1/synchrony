pub mod engine;
pub mod model;

use assert_no_alloc::*;

#[cfg(debug_assertions)] // required when disable_release is set (default)
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AddNode;
    use crate::engine::{AddClip, AddLink, AddTrack};
    use crate::model::Audio;
    use crate::model::flow::Param;
    use crate::model::flow::nodes::lowpass::LowpassFilter;
    use crate::model::project::ProjectData;
    use anyhow::Result;

    use std::sync::Arc;

    use engine::Engine;
    #[test]
    fn it_works() {
        helper();
    }

    fn helper() -> Result<()> {
        // --- 2. Build the project through the real API, not by hand --------
        let mut engine = {
            let project = Arc::new(ProjectData::new());
            Engine::new(project)?
        };
        let master_node_id = engine.project().master_node_id;
        let song_asset = engine.load_asset()?;

        let song_len = {
            let asset = &engine.project().assets[song_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };

        let lowpass = engine.apply(AddNode {
            node: LowpassFilter {
                cutoff_hz: Param::new(1200.0),
            },
        })?;

        engine.apply(AddLink {
            from: (lowpass, LowpassFilter::AUDIO_IN),
            to: (engine.project().master_node_id, 0),
        })?;

        let song_track = engine.apply(AddTrack {
            name: "Song".to_string(),
            kind: Audio,
        })?;

        let song_node = engine
            .project()
            .tracks
            .get(song_track)
            .unwrap()
            .linked_node_id
            .unwrap();

        engine.apply(AddLink {
            from: (song_node, 0),
            to: (master_node_id, 0),
        })?;

        engine.apply(AddClip::<Audio> {
            track: song_track,
            start: engine::tick::Tick(0),
            end: engine::tick::Tick(song_len),
            asset_id: song_asset,
        })?;

        engine
            .playhead
            .store(0, std::sync::atomic::Ordering::Relaxed);

        engine.transport.play();
        println!("Playing... press enter to quit");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        engine.transport.stop();
        Ok(())
    }
}
