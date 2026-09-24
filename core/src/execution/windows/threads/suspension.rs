use super::super::{DispatchError, GuestMemory, synchronization::SyncObjects, thread};
use super::Threads;

impl Threads {
    pub(super) fn suspended(&self, handle: u32) -> bool {
        self.children
            .get(&handle)
            .is_some_and(|context| context.suspend_count != 0)
    }

    pub(in super::super) fn suspend(
        &mut self,
        handle: u32,
        teb: thread::Teb,
        loader_owner: Option<u32>,
        handles: &SyncObjects,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if teb.0 != self.active_teb() || handle == u32::MAX - 1 {
            return Err(DispatchError::Unsupported);
        }
        let Some(context) = self
            .children
            .get_mut(&handle)
            .filter(|_| handles.is_thread(handle))
        else {
            teb.set_last_error(memory, 6)?;
            return Ok(u32::MAX);
        };
        if context.teb == teb.0 || loader_owner == Some(context.teb) {
            return Err(DispatchError::Unsupported);
        }
        let previous = context.suspend_count;
        if previous == 127 {
            teb.set_last_error(memory, 156)?;
            return Ok(u32::MAX);
        }
        context.suspend_count += 1;
        Ok(previous)
    }
}
