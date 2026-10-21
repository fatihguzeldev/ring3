#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SimdState {
    pub mxcsr: u32,
    pub xmm: [[u8; 16]; 8],
}

impl Default for SimdState {
    fn default() -> Self {
        Self {
            mxcsr: 0x1f80,
            xmm: [[0; 16]; 8],
        }
    }
}
