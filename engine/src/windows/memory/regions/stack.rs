use super::*;
use crate::memory::{GuardPage32, Permissions, WordWrite32};

impl VirtualMemory32 {
    pub(crate) fn grow_stack_guard(
        &mut self,
        memory: &mut AddressSpace,
        guard: GuardPage32,
        next: GuestAddress,
        limit: WordWrite32,
    ) -> Result<(), MemoryError32> {
        if !guard.is_current(memory) {
            return Err(MemoryError32::InvalidAddress);
        }
        let current = PageRange::new(guard.address(), 1)?;
        let next_range = PageRange::new(next, 1)?;
        let index = self
            .region(guard.address().0)
            .ok_or(MemoryError32::InvalidAddress)?;
        if next_range.first + 1 != current.first
            || !contains(self.regions[index].range, next_range)
            || self.committed(index, next_range.first)
            || !self.committed(index, current.first)
            || memory.permissions_at(guard.address())? != Permissions::READ_WRITE.with_guard()
        {
            return Err(MemoryError32::InvalidAddress);
        }
        let mut committed = self.replaced_segments(index, current, Some(4))?;
        committed
            .try_reserve(1)
            .map_err(|_| MemoryError32::Allocation)?;
        committed.push(Committed32 {
            range: next_range,
            protection: 4 | PAGE_GUARD,
        });
        normalize(&mut committed);
        self.check_fragments(committed.len(), self.regions[index].committed.len())?;
        memory.apply_private_pages(
            &[
                PrivatePageChange32::Commit {
                    address: next,
                    permissions: Permissions::READ_WRITE.with_guard(),
                },
                guard.clear(),
            ],
            &[(limit.address, &limit.value.to_le_bytes())],
        )?;
        self.regions[index].committed = committed;
        Ok(())
    }
}
