pub mod domain;
mod geometry;
pub mod program;

pub use geometry::{Basis, Degree, PatchInfo, RectPatchInfo, TriPatchInfo};

pub const MAX_SEGMENTS: f32 = 64.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segments([u32; 4]);

impl Segments {
    pub fn new(values: &[u32]) -> Result<Self, PatchError> {
        if !matches!(values.len(), 3 | 4)
            || values.iter().any(|value| {
                let value = f32::from_bits(*value);
                !value.is_finite() || !(1.0..=MAX_SEGMENTS).contains(&value)
            })
        {
            return Err(PatchError);
        }
        let mut result = [0; 4];
        result[..values.len()].copy_from_slice(values);
        Ok(Self(result))
    }

    pub fn uniform(value: u32, triangular: bool) -> Result<Self, PatchError> {
        let values = [value; 4];
        Self::new(&values[..if triangular { 3 } else { 4 }])
    }

    pub fn words(self) -> [u32; 4] {
        self.0
    }

    pub fn validate(self, triangular: bool) -> Result<(), PatchError> {
        Self::new(&self.0[..if triangular { 3 } else { 4 }])?;
        if triangular && self.0[3] != 0 {
            return Err(PatchError);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
