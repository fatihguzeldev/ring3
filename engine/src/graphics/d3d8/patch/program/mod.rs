mod basis;
mod evaluation;
mod inputs;

pub use evaluation::SurfaceSample;
pub use inputs::InputPlan;

use super::{Basis, PatchError, PatchInfo};

pub const PROGRAM_WORDS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceProgram {
    words: [u32; PROGRAM_WORDS],
}

impl SurfaceProgram {
    pub fn compile(info: PatchInfo) -> Result<Self, PatchError> {
        info.vertex_extent()?;
        let mut words = [0; PROGRAM_WORDS];
        match info {
            PatchInfo::Rectangle(info) => {
                let degree = info.degree as u32;
                words[..8].copy_from_slice(&[
                    0,
                    info.start_height * info.stride + info.start_width,
                    info.stride,
                    degree,
                    if info.basis == Basis::Bezier {
                        1
                    } else {
                        info.width - degree
                    },
                    if info.basis == Basis::Bezier {
                        1
                    } else {
                        info.height - degree
                    },
                    info.width,
                    info.height,
                ]);
                for (slot, coefficient) in words[8..44].iter_mut().zip(
                    basis::coefficients(info.basis, degree)
                        .into_iter()
                        .flatten(),
                ) {
                    *slot = coefficient.to_bits();
                }
                info.width.checked_mul(info.height).ok_or(PatchError)?;
            }
            PatchInfo::Triangle(info) => {
                let degree = info.degree as u32;
                words[..8].copy_from_slice(&[1, info.start, 0, degree, 1, 1, info.vertices, 1]);
                let mut cursor = 8;
                for row in 0..=degree {
                    for column in 0..=row {
                        let u = row - column;
                        let v = column;
                        let w = degree - row;
                        let coefficient = basis::factorial(degree)
                            / (basis::factorial(u) * basis::factorial(v) * basis::factorial(w));
                        words[cursor..cursor + 4].copy_from_slice(&[
                            u,
                            v,
                            w,
                            (coefficient as f32).to_bits(),
                        ]);
                        cursor += 4;
                    }
                }
            }
        }
        Ok(Self { words })
    }

    pub fn words(&self) -> &[u32; PROGRAM_WORDS] {
        &self.words
    }
    pub fn triangular(&self) -> bool {
        self.words[0] == 1
    }
    pub fn control_count(&self) -> u32 {
        self.words[6] * self.words[7]
    }
    pub fn source_vertex(&self, index: u32) -> Option<u32> {
        if index >= self.control_count() {
            return None;
        }
        Some(self.words[1] + (index / self.words[6]) * self.words[2] + index % self.words[6])
    }
}

#[cfg(test)]
mod tests;
