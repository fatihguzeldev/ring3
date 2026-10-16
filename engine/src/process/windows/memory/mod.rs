mod calls;
mod context;
mod virtual_calls;

use crate::{
    memory::PageRange,
    windows::memory::{HeapManager32, MemoryError32, MemoryLimits32, VirtualMemory32},
};

pub(in crate::process) struct WindowsMemoryRuntime32 {
    virtual_memory: VirtualMemory32,
    heaps: HeapManager32,
    limits: MemoryLimits32,
}
impl WindowsMemoryRuntime32 {
    #[cfg(test)]
    pub(in crate::process) fn reserve_for_test(
        &mut self,
        memory: &mut crate::memory::AddressSpace,
        range: PageRange,
    ) -> Result<u32, MemoryError32> {
        self.virtual_memory.allocate(
            memory,
            &[],
            range.first as u32 * crate::memory::PAGE_SIZE,
            range.count as u32 * crate::memory::PAGE_SIZE,
            crate::windows::memory::MEM_RESERVE,
            4,
        )
    }
    pub(in crate::process) fn new(limits: MemoryLimits32) -> Result<Self, MemoryError32> {
        Ok(Self {
            virtual_memory: VirtualMemory32::new(limits)?,
            heaps: HeapManager32::new(limits)?,
            limits,
        })
    }
    pub(in crate::process) fn prepare_initial_heap(
        &self,
        memory: &mut crate::memory::AddressSpace,
        external: &[PageRange],
    ) -> Result<Self, MemoryError32> {
        if !self.is_empty() {
            return Err(MemoryError32::Busy);
        }
        let mut candidate = Self::new(self.limits)?;
        candidate
            .heaps
            .ensure_default(&mut candidate.virtual_memory, memory, external)?;
        Ok(candidate)
    }
    pub(in crate::process) fn default_heap(&self) -> Option<u32> {
        self.heaps.default_heap()
    }

    pub(in crate::process) fn empty_same_limits(&self) -> Result<Self, MemoryError32> {
        Self::new(self.limits)
    }
    pub(in crate::process) fn committed_ranges(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.virtual_memory.committed_ranges()
    }
    pub(in crate::process) fn reserve_thread_storage(
        &mut self,
        memory: &mut crate::memory::AddressSpace,
        external: &[PageRange],
        bytes: u32,
    ) -> Result<u32, MemoryError32> {
        self.virtual_memory.allocate(
            memory,
            external,
            0,
            bytes,
            crate::windows::memory::MEM_RESERVE,
            4,
        )
    }
    pub(in crate::process) fn commit_thread_storage(
        &mut self,
        memory: &mut crate::memory::AddressSpace,
        base: u32,
        bytes: u32,
        writes: &[(crate::memory::GuestAddress, &[u8])],
    ) -> Result<u32, MemoryError32> {
        self.virtual_memory.allocate_with_writes(
            memory,
            &[],
            base,
            bytes,
            crate::windows::memory::MEM_COMMIT,
            4,
            writes,
        )
    }
    pub(in crate::process) fn install_thread_pages(
        &mut self,
        memory: &mut crate::memory::AddressSpace,
        base: u32,
        pages: &[crate::memory::PageInitialization<'_>],
        words: &[crate::memory::WordWrite32],
    ) -> Result<(), MemoryError32> {
        self.virtual_memory
            .install_thread_pages(memory, base, pages, words)
    }
    pub(in crate::process) fn grow_stack_guard(
        &mut self,
        memory: &mut crate::memory::AddressSpace,
        guard: crate::memory::GuardPage32,
        next: crate::memory::GuestAddress,
        limit: crate::memory::WordWrite32,
    ) -> Result<(), MemoryError32> {
        self.virtual_memory
            .grow_stack_guard(memory, guard, next, limit)
    }
    pub(in crate::process) fn reservations(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.virtual_memory.reservations()
    }
    pub(in crate::process) fn reservation_count(&self) -> usize {
        self.virtual_memory.reservation_count()
    }
    pub(in crate::process) fn is_empty(&self) -> bool {
        self.virtual_memory.is_empty()
    }
    pub(in crate::process) fn revoke(&mut self, range: PageRange) {
        self.virtual_memory.revoke(range);
        self.heaps.revoke(range);
    }
}

#[cfg(test)]
mod tests;
