mod floating;
mod registers;

use crate::cpu::x86::{SegmentContext, State32, X87State, simd::SimdState};

pub const CONTEXT_SIZE32: usize = 716;
pub const CONTEXT_ARCH32: u32 = 0x0001_0000;
pub const CONTEXT_CONTROL32: u32 = CONTEXT_ARCH32 | 1;
pub const CONTEXT_INTEGER32: u32 = CONTEXT_ARCH32 | 2;
pub const CONTEXT_SEGMENTS32: u32 = CONTEXT_ARCH32 | 4;
pub const CONTEXT_FLOATING32: u32 = CONTEXT_ARCH32 | 8;
pub const CONTEXT_DEBUG32: u32 = CONTEXT_ARCH32 | 0x10;
pub const CONTEXT_EXTENDED32: u32 = CONTEXT_ARCH32 | 0x20;
pub const CONTEXT_FULL32: u32 = CONTEXT_ARCH32 | 7;
pub const CONTEXT_ALL32: u32 = CONTEXT_ARCH32 | 0x3f;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextError32 {
    Length,
    UnsupportedFlags,
    InvalidSegment,
    InvalidFloatingPoint,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextState32 {
    pub state: State32,
    pub segments: SegmentContext,
    pub x87: X87State,
    pub simd: SimdState,
    pub debug: [u32; 6],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Win32Context32 {
    bytes: [u8; CONTEXT_SIZE32],
}
impl Win32Context32 {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ContextError32> {
        let bytes = bytes.try_into().map_err(|_| ContextError32::Length)?;
        let result = Self { bytes };
        validate_flags(result.flags())?;
        if result.selected(0x20) {
            result.extended()?;
        }
        Ok(result)
    }
    pub fn bytes(&self) -> &[u8; CONTEXT_SIZE32] {
        &self.bytes
    }
    pub fn flags(&self) -> u32 {
        self.word(0)
    }
    fn selected(&self, family: u32) -> bool {
        self.flags() & family != 0
    }
    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
    fn write(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
fn validate_flags(flags: u32) -> Result<(), ContextError32> {
    if flags & CONTEXT_ARCH32 == 0 || flags & !(CONTEXT_ARCH32 | 0x3f) != 0 {
        Err(ContextError32::UnsupportedFlags)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests;
