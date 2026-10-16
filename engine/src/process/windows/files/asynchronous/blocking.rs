use super::context::{AsyncProgress32, AsyncStatus32};
use crate::{
    files::FileError32, memory::AddressSpace, process::windows::files::FileRuntime32,
    windows::synchronization::SynchronizationContext32,
};
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn blocking_lock_ready(&self, id: u64) -> bool {
        let Some(operation) = self
            .asynchronous
            .operations
            .iter()
            .find(|operation| operation.id == id)
        else {
            return true;
        };
        if operation.status != AsyncStatus32::Pending || operation.cancel {
            return true;
        }
        let Some(lock) = operation.lock() else {
            return true;
        };
        let Some(open) = operation.target.open() else {
            return true;
        };
        !matches!(
            self.prepare_file_lock(open, operation.offset, lock.length, lock.exclusive),
            Err(FileError32::LockViolation)
        )
    }
    pub(in crate::process::windows::files) fn cancel_blocking_file_wait(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
    ) -> Result<(), FileError32> {
        let Some(id) = self.asynchronous.blocking else {
            return Ok(());
        };
        let index = self
            .asynchronous
            .operations
            .iter()
            .position(|op| op.id == id)
            .ok_or(FileError32::InvalidHandle)?;
        if self.asynchronous.operations[index].status != AsyncStatus32::Pending {
            self.asynchronous.blocking = None;
            return Ok(());
        }
        let old = self.asynchronous.operations[index].cancel;
        self.asynchronous.operations[index].cancel = true;
        match self.progress_io(memory, sync) {
            Ok(AsyncProgress32::Completed { id: completed, .. }) if completed == id => {
                self.asynchronous.blocking = None;
                Ok(())
            }
            Ok(_) => {
                self.asynchronous.operations[index].cancel = old;
                Err(FileError32::Busy)
            }
            Err(error) => {
                self.asynchronous.operations[index].cancel = old;
                Err(error)
            }
        }
    }
}
