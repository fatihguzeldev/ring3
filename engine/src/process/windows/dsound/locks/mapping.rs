use super::{Lock32, LockRegistry32};
use crate::{
    audio::dsound::HResult,
    memory::{
        AddressSpace, GuestAddress, PAGE_SIZE, PageInitialization, PageRange, Permissions,
        WordWrite32,
    },
};

impl LockRegistry32 {
    pub(in crate::process::windows::dsound) fn available_range(
        &self,
        memory: &AddressSpace,
        byte_length: usize,
    ) -> Result<PageRange, HResult> {
        if self.memory.is_some_and(|id| id != memory.identity()) {
            return Err(HResult::INVALID_CALL);
        }
        let pages = byte_length.max(1).div_ceil(PAGE_SIZE as usize);
        memory
            .find_unmapped_range(self.range, pages as u32, PAGE_SIZE)
            .map_err(Into::into)
    }

    pub(in crate::process::windows::dsound) fn install(
        &mut self,
        memory: &mut AddressSpace,
        range: PageRange,
        bytes: &[u8],
        words: &[WordWrite32],
        lock: impl FnOnce(PageRange, u64) -> Lock32,
    ) -> Result<(), HResult> {
        if self.entries.len() == self.limit || self.memory.is_some_and(|id| id != memory.identity())
        {
            return Err(HResult::INVALID_CALL);
        }
        let count = bytes.len().max(1).div_ceil(PAGE_SIZE as usize);
        if range.count != count
            || range.first < self.range.first
            || range.first + range.count > self.range.first + self.range.count
        {
            return Err(HResult::INVALID_PARAM);
        }
        let start = GuestAddress((range.first * PAGE_SIZE as usize) as u32);
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(count)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for index in 0..count {
            let at = index * PAGE_SIZE as usize;
            pages.push(PageInitialization {
                address: GuestAddress(start.0 + index as u32 * PAGE_SIZE),
                permissions: Permissions::READ_WRITE,
                bytes: &bytes[at.min(bytes.len())..(at + PAGE_SIZE as usize).min(bytes.len())],
            });
        }
        memory.install_pages_with_word_writes(&pages, words)?;
        self.entries.push(lock(range, memory.transaction_version()));
        self.memory = Some(memory.identity());
        Ok(())
    }
    pub(in crate::process::windows::dsound) fn validate(
        &self,
        memory: &AddressSpace,
        lock: &Lock32,
    ) -> Result<(), HResult> {
        if self.memory != Some(memory.identity()) {
            return Err(HResult::INVALID_CALL);
        }
        for page in lock.range.first..lock.range.first + lock.range.count {
            let address = GuestAddress((page * PAGE_SIZE as usize) as u32);
            if memory.mapping_version_at(address)? != lock.mapping
                || memory.permissions_at(address)? != Permissions::READ_WRITE
            {
                return Err(HResult::INVALID_CALL);
            }
        }
        Ok(())
    }
    pub(in crate::process::windows::dsound) fn reserve_readonly_page(
        &mut self,
        memory: &mut AddressSpace,
        bytes: &[u8],
    ) -> Result<(GuestAddress, u64), HResult> {
        if bytes.len() > PAGE_SIZE as usize {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let range = self.available_range(memory, bytes.len())?;
        let address = GuestAddress((range.first * PAGE_SIZE as usize) as u32);
        memory.install_pages_with_word_writes(
            &[PageInitialization {
                address,
                permissions: Permissions::READ,
                bytes,
            }],
            &[],
        )?;
        self.memory = Some(memory.identity());
        Ok((address, memory.transaction_version()))
    }
}
