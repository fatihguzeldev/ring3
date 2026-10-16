mod mesh;
mod partition;

use super::{PatchError, Segments};
pub use mesh::DomainMesh;

pub const MAX_VERTICES: usize = 8193;
pub const MAX_INDICES: usize = 48384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeStyle {
    Discrete,
    Continuous,
}
impl TryFrom<u32> for EdgeStyle {
    type Error = PatchError;
    fn try_from(value: u32) -> Result<Self, PatchError> {
        match value {
            0 => Ok(Self::Discrete),
            1 => Ok(Self::Continuous),
            _ => Err(PatchError),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainKey {
    pub triangular: bool,
    pub segments: Segments,
    pub style: EdgeStyle,
}
impl DomainKey {
    pub fn validate(self) -> Result<(), PatchError> {
        self.segments.validate(self.triangular)
    }
}

#[cfg(test)]
mod tests;
