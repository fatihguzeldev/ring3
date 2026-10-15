/// HRESULT values shared by the D3D8 engine and its guest adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HResult(pub u32);

impl HResult {
    pub const OK: Self = Self(0);
    pub const NO_INTERFACE: Self = Self(0x8000_4002);
    pub const OUT_OF_MEMORY: Self = Self(0x8007_000e);
    pub const INVALID_CALL: Self = Self(0x8876_086c);
    pub const NOT_AVAILABLE: Self = Self(0x8876_086a);
    pub const OUT_OF_VIDEO_MEMORY: Self = Self(0x8876_017c);
    pub const NOT_FOUND: Self = Self(0x8876_0866);
    pub const MORE_DATA: Self = Self(0x8876_0867);
    pub const DEVICE_LOST: Self = Self(0x8876_0868);
    pub const DEVICE_NOT_RESET: Self = Self(0x8876_0869);
}
