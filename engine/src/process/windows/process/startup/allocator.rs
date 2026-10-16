use super::ProcessStartup32;
use crate::{memory::PAGE_SIZE, windows::process::ProcessError32};

pub(in crate::process::windows::process) struct StartupArena32 {
    peb: u32,
    revision: u64,
    limit: usize,
    bump: usize,
    free: Vec<(usize, usize)>,
}

impl ProcessStartup32 {
    pub(in crate::process::windows::process) fn prepare_arena(
        &self,
    ) -> Result<StartupArena32, ProcessError32> {
        self.allocation_revision
            .checked_add(1)
            .ok_or(ProcessError32::Capacity)?;
        let mut free = Vec::new();
        free.try_reserve_exact(self.free_blocks.len())
            .map_err(|_| ProcessError32::Allocation)?;
        free.extend_from_slice(&self.free_blocks);
        Ok(StartupArena32 {
            peb: self.peb,
            revision: self.allocation_revision,
            limit: self.range.count * PAGE_SIZE as usize,
            bump: self.bump,
            free,
        })
    }
    pub(in crate::process::windows::process) fn commit_arena(&mut self, arena: StartupArena32) {
        assert_eq!(
            (self.peb, self.allocation_revision),
            (arena.peb, arena.revision)
        );
        self.bump = arena.bump;
        self.free_blocks = arena.free;
        self.allocation_revision += 1;
    }
}
impl StartupArena32 {
    pub(in crate::process::windows::process) fn allocate(
        &mut self,
        length: usize,
        alignment: usize,
    ) -> Result<u32, ProcessError32> {
        if length == 0 || !alignment.is_power_of_two() {
            return Err(ProcessError32::InvalidParameter);
        }
        for index in 0..self.free.len() {
            let (offset, size) = self.free[index];
            let at = offset.div_ceil(alignment) * alignment;
            let end = at.checked_add(length).ok_or(ProcessError32::Capacity)?;
            if end > offset + size {
                continue;
            }
            self.free
                .try_reserve(2)
                .map_err(|_| ProcessError32::Allocation)?;
            self.free.remove(index);
            if at > offset {
                self.free.push((offset, at - offset));
            }
            if end < offset + size {
                self.free.push((end, offset + size - end));
            }
            return Ok(self.peb + at as u32);
        }
        let at = self.bump.div_ceil(alignment) * alignment;
        let end = at
            .checked_add(length)
            .filter(|end| *end <= self.limit)
            .ok_or(ProcessError32::Capacity)?;
        self.bump = end;
        Ok(self.peb + at as u32)
    }
    pub(in crate::process::windows::process) fn release(
        &mut self,
        address: u32,
        length: usize,
    ) -> Result<(), ProcessError32> {
        let offset = address
            .checked_sub(self.peb)
            .ok_or(ProcessError32::InvalidParameter)? as usize;
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > self.limit)
        {
            return Err(ProcessError32::InvalidParameter);
        }
        self.free
            .try_reserve(1)
            .map_err(|_| ProcessError32::Allocation)?;
        self.free.push((offset, length));
        self.free.sort_unstable();
        let mut index = 0;
        while index + 1 < self.free.len() {
            let end = self.free[index].0 + self.free[index].1;
            assert!(
                end <= self.free[index + 1].0,
                "startup allocation released twice"
            );
            if end == self.free[index + 1].0 {
                let next = self.free.remove(index + 1);
                self.free[index].1 += next.1;
            } else {
                index += 1;
            }
        }
        Ok(())
    }
}
