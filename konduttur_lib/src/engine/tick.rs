//! Module for the `Tick` primitive.
/// Atomic unit of time within the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick(pub u64);

impl From<u64> for Tick {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<usize> for Tick {
    fn from(value: usize) -> Self {
        Self(value as u64)
    }
}

impl std::ops::Add for Tick {
    type Output = Tick;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Tick {
    type Output = Tick;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}
