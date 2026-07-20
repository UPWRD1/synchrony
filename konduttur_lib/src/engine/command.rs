use crate::{
    engine::tick::Tick,
    model::{
        Kind, Stored,
        arr::{clip::AudioClipID, track::AudioTrackID},
        flow::{NodeID, SocketIndex},
        project::ProjectData,
    },
};
use anyhow::Result;
pub trait Command {
    type Output;
    fn execute(self, project: &mut ProjectData) -> Result<Self::Output>;
}

pub struct AddTrack<K: Kind> {
    pub name: String,
    pub kind: K,
}

impl<K: Kind> Command for AddTrack<K> {
    type Output = <K::Track as Stored>::Id;
    fn execute(self, project: &mut ProjectData) -> Result<Self::Output> {
        project.add_track::<K>(self.name)
    }
}

pub struct RemoveTrack(pub AudioTrackID);

pub struct AddClip<K: Kind> {
    pub track: <K::Track as Stored>::Id,
    pub start: Tick,
    pub end: Tick,
    pub asset_id: <K::Asset as Stored>::Id,
}

impl<K: Kind> Command for AddClip<K> {
    type Output = <K::Clip as Stored>::Id;
    fn execute(self, project: &mut ProjectData) -> Result<Self::Output> {
        project.add_clip_to_track::<K>(self.track, self.start, self.end, self.asset_id)
    }
}

pub struct MoveClip {
    pub track: AudioTrackID,
    pub clip: AudioClipID,
    pub new_start: Tick,
}

pub struct AddLink {
    pub from: (NodeID, SocketIndex),
    pub to: (NodeID, SocketIndex),
}

struct RemoveLink {
    pub from: (NodeID, SocketIndex),
    pub to: (NodeID, SocketIndex),
}
