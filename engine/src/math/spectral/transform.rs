use super::{AllocationError, Buffer32};
use core::ops::Range;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Plan32 {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
}

pub(crate) struct Workspace32 {
    pub(super) values: Buffer32,
}

impl Plan32 {
    pub(crate) fn new(length: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        Self {
            forward: planner.plan_fft_forward(length),
            inverse: planner.plan_fft_inverse(length),
        }
    }

    pub(crate) fn workspace(&self) -> Result<Workspace32, AllocationError> {
        Ok(Workspace32 {
            values: Buffer32::zeroed(
                self.forward
                    .get_inplace_scratch_len()
                    .max(self.inverse.get_inplace_scratch_len()),
            )?,
        })
    }

    #[inline]
    pub(crate) fn forward(
        &self,
        buffer: &mut Buffer32,
        range: Range<usize>,
        workspace: &mut Workspace32,
    ) {
        self.forward
            .process_with_scratch(&mut buffer.values[range], &mut workspace.values.values);
    }

    #[inline]
    pub(crate) fn inverse(
        &self,
        buffer: &mut Buffer32,
        range: Range<usize>,
        workspace: &mut Workspace32,
    ) {
        self.inverse
            .process_with_scratch(&mut buffer.values[range], &mut workspace.values.values);
    }
}
