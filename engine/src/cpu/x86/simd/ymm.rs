/// the upper halves of YMM0..YMM7; lower halves remain in SimdState.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct YmmUpperState {
    pub registers: [[u8; 16]; 8],
}
