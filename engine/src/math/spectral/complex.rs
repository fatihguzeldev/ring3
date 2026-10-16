use core::ops::Mul;
use rustfft::num_complex::Complex32 as VendorComplex;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Complex32 {
    pub(crate) re: f32,
    pub(crate) im: f32,
}

impl Complex32 {
    #[inline]
    pub(crate) const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    #[inline]
    pub(crate) fn norm_sqr(self) -> f32 {
        self.vendor().norm_sqr()
    }

    #[inline]
    pub(crate) fn conj(self) -> Self {
        Self::from_vendor(self.vendor().conj())
    }

    #[inline]
    pub(super) fn vendor(self) -> VendorComplex {
        VendorComplex::new(self.re, self.im)
    }

    #[inline]
    pub(super) fn from_vendor(value: VendorComplex) -> Self {
        Self::new(value.re, value.im)
    }
}

impl Mul for Complex32 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self::from_vendor(self.vendor() * rhs.vendor())
    }
}

impl Mul<f32> for Complex32 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::from_vendor(self.vendor() * rhs)
    }
}
