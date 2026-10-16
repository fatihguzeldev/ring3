mod creation;
mod mutations;

use super::{
    context::{Handle32, Object32, ObjectKind32, set},
    events::{event_access, normalize_name},
    *,
};
pub use creation::PreparedObjectHandle32;
pub use mutations::PreparedMutexRelease32;

impl SynchronizationContext32 {
    pub(super) fn require_event(&self, slot: usize) -> Result<(), SynchronizationError32> {
        if matches!(
            self.objects[slot].as_ref().map(|object| object.kind),
            Some(ObjectKind32::Event)
        ) {
            Ok(())
        } else {
            Err(SynchronizationError32::InvalidHandle)
        }
    }
    pub fn set_current_thread(&mut self, thread: u32) -> Result<(), SynchronizationError32> {
        if thread == 0 {
            return Err(SynchronizationError32::InvalidWait);
        }
        self.current_thread = thread;
        Ok(())
    }
    pub fn thread_from_handle(&self, handle: u32) -> Result<u32, SynchronizationError32> {
        let index = self.handle_index(handle, 0)?;
        match self.objects[self.handles[index].event]
            .as_ref()
            .unwrap()
            .kind
        {
            ObjectKind32::Thread { id, .. } => Ok(id),
            _ => Err(SynchronizationError32::InvalidHandle),
        }
    }
}
pub(super) fn object_access(
    slot: usize,
    context: &SynchronizationContext32,
    access: u32,
) -> Result<u32, SynchronizationError32> {
    match context.objects[slot].as_ref().unwrap().kind {
        ObjectKind32::Event | ObjectKind32::Semaphore { .. } | ObjectKind32::Timer { .. } => {
            event_access(access)
        }
        ObjectKind32::Mutex { .. } => {
            let mut mapped = access & 0x0fff_ffff;
            if access & 0x1000_0000 != 0 {
                mapped |= 0x001f_0001;
            }
            if access & 0x8000_0000 != 0 {
                mapped |= 0x0002_0001;
            }
            if access & 0x4000_0000 != 0 {
                mapped |= 0x0002_0001;
            }
            if access & 0x2000_0000 != 0 {
                mapped |= 0x0012_0000;
            }
            if mapped & !0x001f_0001 != 0 {
                Err(SynchronizationError32::AccessDenied)
            } else {
                Ok(mapped)
            }
        }
        ObjectKind32::Thread { .. } => thread_access(access),
    }
}

pub(super) fn thread_access(access: u32) -> Result<u32, SynchronizationError32> {
    let mut mapped = access & 0x0fff_ffff;
    if access & 0x1000_0000 != 0 {
        mapped |= 0x001f_ffff;
    }
    if access & 0x8000_0000 != 0 {
        mapped |= 0x0002_0040;
    }
    if access & 0x4000_0000 != 0 {
        mapped |= 0x0002_003b;
    }
    if access & 0x2000_0000 != 0 {
        mapped |= 0x0012_0000;
    }
    if mapped & !0x001f_ffff != 0 {
        Err(SynchronizationError32::AccessDenied)
    } else {
        Ok(mapped)
    }
}
