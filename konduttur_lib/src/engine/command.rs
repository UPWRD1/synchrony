use crate::{
    engine::tick::Tick,
    model::{
        DataKind,
        arr::{clip::ClipID, track::TrackID},
        asset::AssetID,
        flow::{NodeID, SocketIndex},
        project::ProjectData,
    },
};
use anyhow::Result;
pub trait Command {
    type Output;
    fn execute(self, project: &mut ProjectData) -> Result<Self::Output>;
}

pub struct AddTrack {
    pub name: String,
    pub kind: DataKind,
}

impl Command for AddTrack {
    type Output = TrackID;
    fn execute(self, project: &mut ProjectData) -> Result<Self::Output> {
        project.add_track(self.name, self.kind)
    }
}

pub struct RemoveTrack(pub TrackID);

pub struct AddClip {
    pub track: TrackID,
    pub start: Tick,
    pub end: Tick,
    pub asset: AssetID,
}

impl Command for AddClip {
    type Output = ClipID;
    fn execute(self, project: &mut ProjectData) -> Result<Self::Output> {
        project.add_clip_to_track(self.track, self.start, self.end, self.asset)
    }
}

pub struct MoveClip {
    pub track: TrackID,
    pub clip: ClipID,
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
