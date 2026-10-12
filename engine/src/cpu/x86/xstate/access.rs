/// exact standard-format spans used by the supported 32-bit components.
/// a zero selection is the unconditional header; other values are rfbm bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemorySpan {
    pub offset: u32,
    pub width: u32,
    pub selection: u32,
}

impl MemorySpan {
    const fn new(selection: u32, offset: u32, width: u32) -> Self {
        Self {
            offset,
            width,
            selection,
        }
    }
}

pub(crate) const SAVE_SPANS: [MemorySpan; 5] = [
    MemorySpan::new(0, 512, 8),
    MemorySpan::new(1, 0, 24),
    MemorySpan::new(2, 24, 8),
    MemorySpan::new(1, 32, 128),
    MemorySpan::new(2, 160, 128),
];

pub(crate) const RESTORE_SPANS: [MemorySpan; 5] = [
    MemorySpan::new(0, 512, 24),
    MemorySpan::new(1, 0, 24),
    MemorySpan::new(2, 24, 4),
    MemorySpan::new(1, 32, 128),
    MemorySpan::new(2, 160, 128),
];

#[cfg(test)]
#[path = "access/tests.rs"]
mod tests;
