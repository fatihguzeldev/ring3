use super::super::SegmentDescriptorRecord;

/// complete configured IDT image, independent of IDTR readback and guest RAM.
#[derive(Debug, PartialEq, Eq)]
pub struct InterruptDescriptorTable32 {
    limit: u16,
    records: Box<[SegmentDescriptorRecord; 256]>,
}

impl InterruptDescriptorTable32 {
    /// raw slots and any byte limit are authority; gate validation belongs to execution.
    pub fn new(limit: u16, records: Box<[SegmentDescriptorRecord; 256]>) -> Self {
        Self { limit, records }
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    pub fn record(&self, vector: u8) -> &SegmentDescriptorRecord {
        &self.records[usize::from(vector)]
    }
}

#[cfg(test)]
mod tests;
