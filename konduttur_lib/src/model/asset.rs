use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct AssetID;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    #[serde(skip)]
    pub samples: Arc<Vec<f32>>,
    pub gain: f32,
    pub channels: u16,
    pub path: PathBuf,
}
