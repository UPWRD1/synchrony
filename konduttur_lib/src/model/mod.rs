use crate::{engine::tick::Tick, model::project::Project};

pub mod arr;
pub mod asset;
pub mod flow;
pub mod project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Audio,
    Midi,
    Cv,
}

impl DataKind {
    pub fn can_connect_to(self, dest: Self) -> bool {
        self == dest || (self == Self::Audio && dest == Self::Cv)
    }
}

pub trait Renderable {
    fn render(&self, proj: &Project, buf: &mut [f32], block_start: Tick, channels: u16);
}
