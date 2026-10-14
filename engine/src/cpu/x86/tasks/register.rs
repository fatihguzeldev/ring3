use super::{TaskFormat, TaskReturnError};
use crate::cpu::x86::SegmentDescriptorRecord;

/// explicit hidden tr cache; live gdt edits never rewrite its cached base or limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskRegister {
    selector: u16,
    descriptor: SegmentDescriptorRecord,
    format: TaskFormat,
}

impl TaskRegister {
    /// validate a supplied cached image. this is not ltr execution or a guest-memory read.
    /// a cached short limit is retained so backlink access can report its own ts fault.
    pub fn from_cached(
        selector: u16,
        descriptor: SegmentDescriptorRecord,
    ) -> Result<Self, TaskReturnError> {
        let error = u32::from(selector & !3);
        if selector & !3 == 0 || selector & 4 != 0 {
            return Err(TaskReturnError::InvalidTss(error));
        }
        let format = descriptor_format(descriptor)
            .filter(|_| descriptor.descriptor_type() & 2 != 0)
            .ok_or(TaskReturnError::InvalidTss(error))?;
        if !descriptor.present() {
            return Err(TaskReturnError::NotPresent(error));
        }
        Ok(Self::admitted(selector, descriptor, format))
    }

    pub(super) const fn admitted(
        selector: u16,
        descriptor: SegmentDescriptorRecord,
        format: TaskFormat,
    ) -> Self {
        Self {
            selector,
            descriptor,
            format,
        }
    }

    pub const fn selector(self) -> u16 {
        self.selector
    }
    pub const fn descriptor(self) -> SegmentDescriptorRecord {
        self.descriptor
    }
    pub const fn format(self) -> TaskFormat {
        self.format
    }
    pub fn base(self) -> u32 {
        let bytes = self.descriptor.to_bytes();
        u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[7]])
    }
    pub fn limit(self) -> u32 {
        let limit = self.descriptor.raw_limit();
        if self.descriptor.granularity() {
            (limit << 12) | 0xfff
        } else {
            limit
        }
    }
}

pub(super) fn descriptor_format(record: SegmentDescriptorRecord) -> Option<TaskFormat> {
    if record.code_data() {
        return None;
    }
    match record.descriptor_type() {
        1 | 3 => Some(TaskFormat::Word),
        9 | 11 => Some(TaskFormat::Dword),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
