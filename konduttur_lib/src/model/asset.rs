use std::sync::Arc;

use slotmap::new_key_type;

new_key_type! {
    pub struct AssetID;
}

#[derive(Debug, Clone)]
pub struct Asset {
    pub samples: Arc<Vec<f32>>,
    pub gain: f32,
    pub channels: u16,
}
