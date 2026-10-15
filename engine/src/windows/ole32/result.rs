#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HResult32(pub u32);
impl HResult32 {
    pub const OK: Self = Self(0);
    pub const FALSE: Self = Self(1);
    pub const NO_INTERFACE: Self = Self(0x8000_4002);
    pub const POINTER: Self = Self(0x8000_4003);
    pub const INVALID_ARGUMENT: Self = Self(0x8007_0057);
    pub const OUT_OF_MEMORY: Self = Self(0x8007_000e);
    pub const UNEXPECTED: Self = Self(0x8000_ffff);
    pub const CHANGED_MODE: Self = Self(0x8001_0106);
    pub const NOT_INITIALIZED: Self = Self(0x8004_01f0);
    pub const CLASS_NOT_REGISTERED: Self = Self(0x8004_0154);
    pub const CLASS_NOT_AVAILABLE: Self = Self(0x8004_0111);
    pub const NO_AGGREGATION: Self = Self(0x8004_0110);
}
