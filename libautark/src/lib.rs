pub mod engine;
pub mod model;

// use assert_no_alloc::*;

// #[cfg(debug_assertions)] // required when disable_release is set (default)
// #[global_allocator]
// static A: AllocDisabler = AllocDisabler;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AddClip, AddLink, AddTrack};
    use crate::engine::{AddNode, AddNodeInput};
    use crate::model::Audio;
    use crate::model::flow::nodes::biquad_filter::BiquadFilter;
    use crate::model::flow::nodes::sum::Sum;
    use crate::model::project::ProjectData;
    use anyhow::Result;

    use std::sync::Arc;

    use engine::Engine;
    #[test]
    fn it_works() {
        helper().unwrap();
    }

    fn helper() -> Result<()> {
        let mut engine = {
            let project = Arc::new(ProjectData::new());
            Engine::new(project)?
        };
        let master_node_id = engine.project().master_node_id;

        let master_in = engine.project().graph.inputs_of(master_node_id)[0];

        let song_asset = engine.load_asset("./assets/AUDIO_4892.mp3")?;

        let song_len = {
            let asset = &engine.project().assets[song_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };

        let filter1 = engine.apply(AddNode {
            node: BiquadFilter::new(
                engine.channels(),
                model::flow::nodes::biquad_filter::FilterType::HighPass,
                engine.sample_rate(),
                1600.0,
                BiquadFilter::BUTTERWORTH_Q,
                0.0,
            ),
        })?;

        let filter1_in = engine.project().graph.inputs_of(filter1)[0];
        let filter1_out = engine.project().graph.outputs_of(filter1)[0];

        let master_sum = engine.apply(AddNode {
            node: Sum::<Audio>::new(),
        })?;

        let master_sum_in0 = engine.apply(AddNodeInput::<Audio>::new(master_sum))?;

        let master_sum_out = engine.project().graph.outputs_of(master_sum)[0];

        engine.apply(AddLink {
            from: filter1_out,
            to: master_sum_in0,
        })?;

        engine.apply(AddLink {
            from: master_sum_out,
            to: master_in,
        })?;

        let (song_track, song_node) = engine.apply(AddTrack {
            name: "Song".to_string(),
            kind: Audio,
            channels: engine.channels(),
        })?;
        let song_out = engine.project().graph.outputs_of(song_node)[0];

        engine.apply(AddLink {
            from: song_out,
            to: filter1_in,
        })?;

        engine.apply(AddClip::<Audio> {
            track: song_track,
            start: engine::tick::Tick(0),
            end: engine::tick::Tick(song_len),
            asset_id: song_asset,
        })?;

        let clap_asset = engine.load_asset("./assets/clap.mp3")?;

        let clap_len = {
            let asset = &engine.project().assets[clap_asset];
            asset.samples.len() as u64 / asset.channels as u64
        };

        let (clap_track, clap_node) = engine.apply(AddTrack {
            name: "Clap".to_string(),
            kind: Audio,
            channels: engine.channels(),
        })?;

        engine.apply(AddClip::<Audio> {
            track: clap_track,
            start: engine::tick::Tick(1000),
            end: engine::tick::Tick(clap_len),
            asset_id: clap_asset,
        })?;

        let clap_out = engine.project().graph.outputs_of(clap_node)[0];

        let master_sum_in1 = engine.apply(AddNodeInput::<Audio>::new(master_sum))?;

        engine.apply(AddLink {
            from: clap_out,
            to: master_sum_in1,
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
