use std::marker::PhantomData;

use crate::{
    engine::{SlotIndex, bbp::PoolExecutor, tick::Tick},
    model::{
        Audio, DataKind, Kind, Renderable, Stored,
        flow::{Node, Socket},
        project::ProjectData,
    },
};

#[derive(Debug, Clone)]
pub struct TrackReader<K: Kind> {
    kind: PhantomData<K>,
    id: <K::Track as Stored>::Id,
}

impl<K: Kind> TrackReader<K> {
    pub fn new(id: <K::Track as Stored>::Id) -> Self {
        Self {
            kind: PhantomData,
            id,
        }
    }
}

impl TrackReader<Audio> {
    const OUTPUTS: &'static [Socket] = &[Socket {
        name: "audio out",
        kind: DataKind::Audio,
        visible: true,
    }];
}

impl Node for TrackReader<Audio> {
    fn process(
        &self,
        project: &ProjectData,
        pool: &mut PoolExecutor,
        block_start: Tick,
        channels: u16,
        _: &[SlotIndex],
        outputs: &[SlotIndex],
    ) {
        if let Some(track) = project.tracks.get(self.id) {
            let output_buf = pool.get_output(outputs[0]);
            track.render(project, output_buf, block_start, channels);
        }
    }

    fn inputs(&self) -> &[Socket] {
        &[]
    }

    fn outputs(&self) -> &[Socket] {
        Self::OUTPUTS
    }
}
