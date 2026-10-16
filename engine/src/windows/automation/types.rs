#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomationLimits32 {
    pub objects: usize,
    pub bytes: usize,
    pub dimensions: usize,
    pub nesting: usize,
}
impl Default for AutomationLimits32 {
    fn default() -> Self {
        Self {
            objects: 8192,
            bytes: 16 * 1024 * 1024,
            dimensions: 32,
            nesting: 32,
        }
    }
}
impl AutomationLimits32 {
    pub(crate) fn validate(self) -> Result<(), AutomationError32> {
        if self.objects == 0
            || self.objects > 1_048_576
            || self.bytes == 0
            || self.bytes > 256 * 1024 * 1024
            || self.dimensions == 0
            || self.dimensions > 64
            || self.nesting == 0
            || self.nesting > 128
        {
            Err(AutomationError32::InvalidParameter)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationError32 {
    InvalidParameter,
    BadVariantType,
    TypeMismatch,
    Overflow,
    ArrayLocked,
    BadIndex,
    Allocation,
    Capacity,
    Unexpected,
    NotImplemented,
    Memory(crate::memory::MemoryError),
}
impl AutomationError32 {
    pub fn hresult(self) -> u32 {
        match self {
            Self::InvalidParameter => 0x80070057,
            Self::BadVariantType => 0x80020008,
            Self::TypeMismatch => 0x80020005,
            Self::Overflow => 0x8002000a,
            Self::ArrayLocked => 0x8002000d,
            Self::BadIndex => 0x8002000b,
            Self::Allocation | Self::Capacity => 0x8007000e,
            Self::Unexpected => 0x8000ffff,
            Self::NotImplemented => 0x80004001,
            Self::Memory(_) => 0x800703e6,
        }
    }
}
