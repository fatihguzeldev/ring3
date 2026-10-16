use super::GdiContext32;
use crate::windows::user32::SystemColors32;

const SYSTEM_BRUSH_BASE: u32 = 0x7f00_0200;

pub(super) fn system_brush_index(handle: u32) -> Option<u32> {
    handle
        .checked_sub(SYSTEM_BRUSH_BASE)
        .filter(|index| SystemColors32::supports(*index))
}

impl GdiContext32 {
    pub fn system_brush(index: u32) -> Option<u32> {
        SystemColors32::supports(index).then(|| SYSTEM_BRUSH_BASE + index)
    }
}
