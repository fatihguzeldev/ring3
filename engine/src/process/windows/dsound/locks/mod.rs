mod mapping;
mod publication;

use crate::{audio::dsound::LockSpan, memory::PageRange, windows::com::ComObjectId32};

pub(super) struct Lock32 {
    pub object: ComObjectId32,
    pub span: LockSpan,
    pub range: PageRange,
    pub mapping: u64,
    pub first: u32,
    pub second: u32,
    pub original: Option<Vec<u8>>,
}
pub(super) struct LockRegistry32 {
    pub range: PageRange,
    pub next: usize,
    pub entries: Vec<Lock32>,
    pub memory: Option<u64>,
    pub limit: usize,
}
impl LockRegistry32 {
    pub(in crate::process::windows::dsound) fn new(
        range: PageRange,
        limit: u32,
    ) -> Result<Self, super::DSoundError32> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(limit as usize)
            .map_err(|_| super::DSoundError32::Allocation)?;
        Ok(Self {
            range,
            next: range.first,
            entries,
            memory: None,
            limit: limit as usize,
        })
    }
}
