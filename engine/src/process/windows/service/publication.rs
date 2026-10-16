use super::WindowsServiceCleanup32;
use crate::{
    process::{EngineInstance, calls::call::PreparedCall32},
    windows::ThreadState32,
};

impl EngineInstance {
    pub(in crate::process) fn defer_guard_service_call(
        &mut self,
        prepared: PreparedCall32,
    ) -> Option<PreparedCall32> {
        if let Some(service) = self.windows_service.as_mut()
            && self
                .memory
                .as_ref()
                .is_some_and(|memory| memory.guard_service_fault().is_some())
        {
            service.returned = Some(prepared);
            None
        } else {
            Some(prepared)
        }
    }

    pub(in crate::process::windows) fn defer_guard_service_thread(
        &mut self,
        thread: ThreadState32,
    ) -> bool {
        if let Some(service) = self.windows_service.as_mut()
            && self
                .memory
                .as_ref()
                .is_some_and(|memory| memory.guard_service_fault().is_some())
        {
            service.thread = Some(thread);
            true
        } else {
            false
        }
    }

    pub(in crate::process) fn defer_guard_service_cleanup(
        &mut self,
        cleanup: WindowsServiceCleanup32,
    ) -> bool {
        if let Some(service) = self.windows_service.as_mut()
            && self
                .memory
                .as_ref()
                .is_some_and(|memory| memory.guard_service_fault().is_some())
        {
            service.cleanup |= cleanup as u8;
            true
        } else {
            false
        }
    }
}
