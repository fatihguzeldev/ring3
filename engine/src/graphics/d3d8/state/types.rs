use crate::math::{
    Rounding,
    ieee::{self, BinaryFormat, BinaryOperation},
};

use super::StateError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Matrix(pub [u32; 16]);

impl Matrix {
    pub const IDENTITY: Self = Self([
        0x3f80_0000,
        0,
        0,
        0,
        0,
        0x3f80_0000,
        0,
        0,
        0,
        0,
        0x3f80_0000,
        0,
        0,
        0,
        0,
        0x3f80_0000,
    ]);

    pub fn multiply(self, rhs: Self) -> Self {
        let mut result = [0; 16];
        for row in 0..4 {
            for column in 0..4 {
                let mut sum = 0;
                for term in 0..4 {
                    let product = ieee::binary(
                        BinaryOperation::Multiply,
                        BinaryFormat::Binary32,
                        u64::from(self.0[row * 4 + term]),
                        u64::from(rhs.0[term * 4 + column]),
                        Rounding::Nearest,
                    )
                    .bits;
                    sum = ieee::binary(
                        BinaryOperation::Add,
                        BinaryFormat::Binary32,
                        sum,
                        product,
                        Rounding::Nearest,
                    )
                    .bits;
                }
                result[row * 4 + column] = sum as u32;
            }
        }
        Self(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub min_z: u32,
    pub max_z: u32,
}

impl Viewport {
    pub(super) fn validate(self, width: u32, height: u32) -> Result<(), StateError> {
        let min = f32::from_bits(self.min_z);
        let max = f32::from_bits(self.max_z);
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(self.width).is_none_or(|end| end > width)
            || self
                .y
                .checked_add(self.height)
                .is_none_or(|end| end > height)
            || !min.is_finite()
            || !max.is_finite()
            || !(0.0..=1.0).contains(&min)
            || !(0.0..=1.0).contains(&max)
            || min > max
        {
            return Err(StateError::InvalidRange);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Material {
    pub diffuse: [u32; 4],
    pub ambient: [u32; 4],
    pub specular: [u32; 4],
    pub emissive: [u32; 4],
    pub power: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Light {
    pub kind: u32,
    pub diffuse: [u32; 4],
    pub specular: [u32; 4],
    pub ambient: [u32; 4],
    pub position: [u32; 3],
    pub direction: [u32; 3],
    pub range: u32,
    pub falloff: u32,
    pub attenuation: [u32; 3],
    pub theta: u32,
    pub phi: u32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            kind: 3,
            diffuse: [0x3f80_0000, 0x3f80_0000, 0x3f80_0000, 0],
            specular: [0; 4],
            ambient: [0; 4],
            position: [0; 3],
            direction: [0, 0, 0x3f80_0000],
            range: 0,
            falloff: 0,
            attenuation: [0; 3],
            theta: 0,
            phi: 0,
        }
    }
}

impl Light {
    pub(super) fn validate(self) -> Result<(), StateError> {
        if !(1..=3).contains(&self.kind) {
            return Err(StateError::InvalidLight);
        }
        if self.kind != 3
            && self.attenuation.iter().any(|bits| {
                let value = f32::from_bits(*bits);
                !value.is_finite() || value < 0.0
            })
        {
            return Err(StateError::InvalidLight);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamBinding {
    pub resource: u32,
    pub stride: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightState {
    pub index: u32,
    pub light: Light,
    pub enabled: bool,
}
