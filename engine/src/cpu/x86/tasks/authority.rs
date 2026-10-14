use super::{TaskRegister, TaskReturnError};
use crate::cpu::x86::SegmentDescriptorRecord;

/// sparse supplied gdt bytes: none is unknown, some(zero) is an explicit invalid descriptor.
#[derive(Debug)]
pub struct TaskDescriptorTable<'a> {
    limit: u16,
    records: &'a [Option<SegmentDescriptorRecord>],
}

impl<'a> TaskDescriptorTable<'a> {
    pub const fn new(limit: u16, records: &'a [Option<SegmentDescriptorRecord>]) -> Self {
        Self { limit, records }
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    pub(super) fn record(&self, selector: u16) -> Result<SegmentDescriptorRecord, TaskReturnError> {
        self.records
            .get(usize::from(selector >> 3))
            .copied()
            .flatten()
            .ok_or(TaskReturnError::MissingAuthority)
    }
}

/// explicit early-return sources, independent of visible str/gdtr query readback.
/// current_bytes start at the cached current tr base; no live descriptor lookup supplies it.
#[derive(Debug)]
pub struct TaskAuthority<'a> {
    current: TaskRegister,
    descriptors: TaskDescriptorTable<'a>,
    current_bytes: &'a [u8],
}

impl<'a> TaskAuthority<'a> {
    pub const fn new(
        current: TaskRegister,
        descriptors: TaskDescriptorTable<'a>,
        current_bytes: &'a [u8],
    ) -> Self {
        Self {
            current,
            descriptors,
            current_bytes,
        }
    }

    pub const fn current(&self) -> TaskRegister {
        self.current
    }

    pub(super) const fn descriptors(&self) -> &TaskDescriptorTable<'a> {
        &self.descriptors
    }

    pub fn backlink(&self) -> Result<u16, TaskReturnError> {
        if self.current.limit() < 1 {
            return Err(TaskReturnError::InvalidTss(u32::from(
                self.current.selector() & !3,
            )));
        }
        let bytes = self
            .current_bytes
            .get(..2)
            .ok_or(TaskReturnError::MissingAuthority)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
}

#[cfg(test)]
mod tests;
