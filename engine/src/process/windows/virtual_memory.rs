use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
    windows::{
        CallFrame32, ThreadState32,
        memory::{MemoryError32, protection_value},
    },
};

impl EngineInstance {
    pub(in crate::process) fn complete_virtual_alloc(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        size: u32,
        permissions: Permissions,
    ) -> Result<(), HostError> {
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let result = self.memory_allocate(&[0, size, 0x3000, protection_value(permissions)]);
        self.publish_legacy_virtual_result(frame, thread, result)
    }
    pub(in crate::process) fn complete_virtual_free(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        address: u32,
    ) -> Result<(), HostError> {
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let result = self.memory_free(&[address, 0, 0x8000]);
        self.publish_legacy_virtual_result(frame, thread, result)
    }
    pub(in crate::process) fn complete_virtual_protect(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        range: PageRange,
        permissions: Permissions,
        old_output: GuestAddress,
    ) -> Result<(), HostError> {
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let result = self.memory_protect(&[
            range.first as u32 * crate::memory::PAGE_SIZE,
            range.count as u32 * crate::memory::PAGE_SIZE,
            protection_value(permissions),
            old_output.0,
        ]);
        self.publish_legacy_virtual_result(frame, thread, result)
    }
    fn publish_legacy_virtual_result(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        result: Result<u32, MemoryError32>,
    ) -> Result<(), HostError> {
        let (value, thread) = match result {
            Ok(value) => (value, thread),
            Err(MemoryError32::Exhausted) => {
                return Err(HostError::Memory(
                    crate::memory::MemoryError::VersionExhausted,
                ));
            }
            Err(error) => (0, thread.with_last_error(error.win32())),
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        self.publish_windows_thread(thread)?;
        self.publish_prepared_call(prepared);
        Ok(())
    }
    pub(in crate::process) fn revoke_virtual_allocations(&mut self, range: PageRange) {
        self.windows_memory.revoke(range);
    }
}

#[cfg(test)]
#[path = "virtual_memory/tests.rs"]
mod tests;
