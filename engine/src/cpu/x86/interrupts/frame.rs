use super::cause::EventOrigin32;
use crate::cpu::x86::{FarWidth, StackPointerSize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInput32 {
    pub eip: u32,
    pub esp: u32,
    pub flags: u32,
    pub cs: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventFrame32 {
    offsets: [u32; 4],
    values: [u32; 4],
    count: usize,
    slot: u32,
    esp: u32,
    flags: u32,
    info: u32,
}
impl EventFrame32 {
    pub(crate) fn new(
        origin: EventOrigin32,
        input: FrameInput32,
        width: FarWidth,
        stack32: bool,
        interrupt: bool,
    ) -> Self {
        let slot = width.slot_bytes();
        let count = if origin.error_code().is_some() { 4 } else { 3 };
        let pointer = StackPointerSize::new(stack32);
        let offsets = std::array::from_fn(|index| {
            pointer.offset(pointer.advance(input.esp, -(((index + 1) as u32 * slot) as i32)))
        });
        let values = [
            origin.saved_flags(input.flags),
            u32::from(input.cs),
            width.offset(input.eip),
            origin.error_code().unwrap_or(0),
        ];
        Self {
            offsets,
            values,
            count,
            slot,
            esp: pointer.advance(input.esp, -((count as u32 * slot) as i32)),
            flags: input.flags & !(0x34100 | if interrupt { 0x200 } else { 0 }),
            info: slot
                | if interrupt { 0x100 } else { 0 }
                | if origin.error_code().is_some() {
                    0x200
                } else {
                    0
                },
        }
    }
    pub(crate) fn offsets(&self) -> &[u32] {
        &self.offsets[..self.count]
    }
    pub(crate) fn values(&self) -> &[u32] {
        &self.values[..self.count]
    }
    pub(crate) const fn slot(&self) -> u32 {
        self.slot
    }
    pub(crate) const fn esp(&self) -> u32 {
        self.esp
    }
    pub(crate) const fn flags(&self) -> u32 {
        self.flags
    }
    pub(crate) const fn info(&self) -> u32 {
        self.info
    }
}

#[cfg(test)]
mod tests;
