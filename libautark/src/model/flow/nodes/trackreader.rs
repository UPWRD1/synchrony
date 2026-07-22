use std::{borrow::Cow, marker::PhantomData};

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
    channels: u16,
    kind: PhantomData<K>,
    id: <K::Track as Stored>::Id,
}

impl<K: Kind> TrackReader<K> {
    pub fn new(id: <K::Track as Stored>::Id, channels: u16) -> Self {
        Self {
            kind: PhantomData,
            id,
            channels,
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

pub struct TrackReaderState {
    // block_start: Tick,
}

impl Node for TrackReader<Audio> {
    type State = TrackReaderState;

    fn init_state(&self) -> Self::State {
        TrackReaderState {}
    }

    fn process(
        &self,
        pool: &mut PoolExecutor,
        _state: &mut Self::State,
        project: &ProjectData,
        block_start: Tick,
        _: &[SlotIndex],
        outputs: &[SlotIndex],
    ) {
        if let Some(track) = project.tracks.get(self.id) {
            let output_buf = pool.get_output(outputs[0]);
            track.render(project, output_buf, block_start, self.channels);
        }
    }

    fn inputs(&self) -> Cow<'_, [Socket]> {
        Cow::Borrowed(&[])
    }

    fn input(&mut self, _: crate::model::flow::SocketIndex) -> Option<&Socket> {
        None
    }

    fn outputs(&self) -> Cow<'_, [Socket]> {
        Cow::Borrowed(Self::OUTPUTS)
    }
}
