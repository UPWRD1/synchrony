use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

use crate::model::{Audio, Kind, Stored};

new_key_type! {
    pub struct AudioAssetID;
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum AssetState<Data> {
    #[default]
    Pending,
    #[serde(skip)]
    Ready(Data),
    Failed,
}

pub trait Asset<K: Kind> {
    type Data;
    fn new(path: PathBuf) -> Self;
    fn state(&self) -> &AssetState<Self::Data>;
    fn state_mut(&mut self) -> &mut AssetState<Self::Data>;
    fn data(&self) -> Option<&Self::Data> {
        match self.state() {
            AssetState::Ready(data) => Some(data),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAssetData {
    #[serde(skip)]
    pub samples: Arc<Vec<f32>>,
    pub channels: u16,
    pub sample_rate: u32,
    pub gain: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAsset {
    #[serde(skip)]
    pub state: AssetState<AudioAssetData>,
    pub path: PathBuf,
}

impl Stored for AudioAsset {
    type Id = AudioAssetID;

    fn access(project: &super::project::ProjectData) -> &slotmap::SlotMap<Self::Id, Self> {
        &project.assets
    }

    fn access_mut(
        project: &mut super::project::ProjectData,
    ) -> &mut slotmap::SlotMap<Self::Id, Self> {
        &mut project.assets
    }
}

impl Asset<Audio> for AudioAsset {
    type Data = AudioAssetData;
    fn state(&self) -> &AssetState<Self::Data> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut AssetState<Self::Data> {
        &mut self.state
    }

    fn new(path: PathBuf) -> Self {
        Self {
            state: AssetState::Pending,
            path,
        }
    }
}
