use super::{AllocationError, Complex32};
use core::ops::Range;
use rustfft::num_complex::Complex32 as VendorComplex;

pub(crate) struct Buffer32 {
    pub(super) values: Vec<VendorComplex>,
}

impl Buffer32 {
    pub(crate) const VALUE_BYTES: usize = core::mem::size_of::<VendorComplex>();

    pub(crate) fn zeroed(count: usize) -> Result<Self, AllocationError> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| AllocationError)?;
        values.resize(count, VendorComplex::default());
        Ok(Self { values })
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> Complex32 {
        Complex32::from_vendor(self.values[index])
    }

    #[inline]
    pub(crate) fn set(&mut self, index: usize, value: Complex32) {
        self.values[index] = value.vendor();
    }

    #[inline]
    pub(crate) fn add(&mut self, index: usize, value: Complex32) {
        self.values[index] += value.vendor();
    }

    #[inline]
    pub(crate) fn scale(&mut self, index: usize, value: f32) {
        self.values[index] *= value;
    }

    #[inline]
    pub(crate) fn divide(&mut self, range: Range<usize>, divisor: f32) {
        for value in &mut self.values[range] {
            *value /= divisor;
        }
    }

    #[inline]
    pub(crate) fn clear(&mut self) {
        self.values.fill(VendorComplex::default());
    }

    #[inline]
    pub(crate) fn clear_range(&mut self, range: Range<usize>) {
        self.values[range].fill(VendorComplex::default());
    }
}
