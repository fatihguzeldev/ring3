use super::*;
use crate::{
    memory::{GuardPage32, GuestAddress, PAGE_SIZE, WordWrite32},
    process::{EngineInstance, HostError},
};

pub(in crate::process) struct PreparedStackOverflow32 {
    thread: u32,
    previous: PageRange,
    next: PageRange,
    limit: WordWrite32,
}
impl PreparedStackOverflow32 {
    pub(in crate::process) fn limit(&self) -> WordWrite32 {
        self.limit
    }
}
pub(in crate::process) enum StackGuardOutcome32 {
    NotStack,
    Grown,
    Overflow(PreparedStackOverflow32),
}

impl EngineInstance {
    pub(in crate::process) fn grow_windows_stack_guard(
        &mut self,
        guard: GuardPage32,
    ) -> Result<StackGuardOutcome32, HostError> {
        let index = self
            .threads
            .threads
            .iter()
            .position(|record| record.id == self.threads.current)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        let record = &self.threads.threads[index];
        let Some((stack, reserved, teb)) = record
            .stack
            .zip(record.stack_reserved)
            .zip(record.teb)
            .map(|((stack, reserved), teb)| (stack, reserved, teb))
        else {
            return Ok(StackGuardOutcome32::NotStack);
        };
        let page = (guard.address().0 / PAGE_SIZE) as usize;
        if page + 1 != stack.first || !reserved.indices().contains(&page) {
            return Ok(StackGuardOutcome32::NotStack);
        }
        if self
            .memory()?
            .permissions_at(guard.address())
            .map_err(HostError::Memory)?
            != crate::memory::Permissions::READ_WRITE.with_guard()
        {
            return Ok(StackGuardOutcome32::NotStack);
        }
        let next = PageRange {
            first: page,
            count: stack.count + 1,
        };
        let limit = WordWrite32 {
            address: GuestAddress(teb + 8),
            value: guard.address().0,
        };
        if page == reserved.first {
            return Ok(StackGuardOutcome32::Overflow(PreparedStackOverflow32 {
                thread: record.id,
                previous: stack,
                next,
                limit,
            }));
        }
        self.windows_memory
            .grow_stack_guard(
                self.memory.as_mut().ok_or(HostError::InvalidRequest)?,
                guard,
                GuestAddress(guard.address().0 - PAGE_SIZE),
                limit,
            )
            .map_err(HostError::WindowsMemory)?;
        self.threads.threads[index].stack = Some(next);
        Ok(StackGuardOutcome32::Grown)
    }
}

impl ThreadRuntime32 {
    pub(in crate::process) fn commit_stack_overflow(&mut self, receipt: PreparedStackOverflow32) {
        let record = self
            .threads
            .iter_mut()
            .find(|record| record.id == self.current)
            .expect("prepared active stack owner");
        assert_eq!(record.id, receipt.thread);
        assert_eq!(record.stack, Some(receipt.previous));
        record.stack = Some(receipt.next);
    }
}
