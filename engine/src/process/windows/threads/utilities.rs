use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::thread::ThreadApi32 as Api,
};

impl EngineInstance {
    pub(super) fn open_windows_thread(
        &mut self,
        id: u32,
        access: u32,
        inherit: bool,
    ) -> Result<u32, HostError> {
        let info = self.threads.info(id).map_err(HostError::Thread)?;
        if info.state == ThreadState32::Terminated && !self.synchronization()?.has_thread_object(id)
        {
            return Err(HostError::Thread(ThreadError32::InvalidHandle));
        }
        let prepared = self
            .synchronization()?
            .prepare_open_thread(id, access, inherit, info.state == ThreadState32::Terminated)
            .map_err(HostError::Synchronization)?;
        Ok(self.synchronization_mut()?.commit_object_handle(prepared))
    }
    pub(super) fn resolve_query_thread(&self, handle: u32) -> Result<u32, HostError> {
        match self.resolve_thread_handle(handle, 0x40) {
            Err(HostError::Synchronization(
                crate::windows::synchronization::SynchronizationError32::AccessDenied,
            )) => self.resolve_thread_handle(handle, 0x800),
            result => result,
        }
    }
    pub(super) fn execute_thread_utility(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<u32, HostError> {
        if api == Api::DisableThreadLibraryCalls {
            self.disable_windows_thread_notifications(args[0])
                .map_err(|error| match error {
                    HostError::Module(_) => HostError::Thread(ThreadError32::InvalidParameter),
                    other => other,
                })?;
            return Ok(1);
        }
        let id = if api == Api::SetThreadPriority {
            match self.resolve_thread_handle(args[0], 0x20) {
                Err(HostError::Synchronization(
                    crate::windows::synchronization::SynchronizationError32::AccessDenied,
                )) => self.resolve_thread_handle(args[0], 0x400)?,
                other => other?,
            }
        } else {
            self.resolve_query_thread(args[0])?
        };
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == id)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        match api {
            Api::GetThreadPriority => Ok(self.threads.threads[index].priority as u32),
            Api::SetThreadPriority => {
                if self.threads.threads[index].state == ThreadState32::Terminated {
                    return Err(HostError::Thread(ThreadError32::AccessDenied));
                }
                let priority = args[1] as i32;
                if !matches!(priority, -15 | -2 | -1 | 0 | 1 | 2 | 15) {
                    return Err(HostError::Thread(ThreadError32::InvalidParameter));
                }
                self.threads.threads[index].priority = priority;
                Ok(1)
            }
            Api::GetThreadTimes => {
                let thread = &self.threads.threads[index];
                let values = [
                    thread.created_filetime,
                    thread.exited_filetime,
                    0,
                    thread.user_ns / 100,
                ];
                for &address in &args[1..5] {
                    self.memory()?
                        .check_span(GuestAddress(address), 8, Access::Write)
                        .map_err(HostError::Memory)?;
                }
                let bytes = values.map(u64::to_le_bytes);
                self.memory
                    .as_mut()
                    .unwrap()
                    .write_spans(&[
                        (GuestAddress(args[1]), &bytes[0]),
                        (GuestAddress(args[2]), &bytes[1]),
                        (GuestAddress(args[3]), &bytes[2]),
                        (GuestAddress(args[4]), &bytes[3]),
                    ])
                    .map_err(HostError::Memory)?;
                Ok(1)
            }
            _ => Err(HostError::Thread(ThreadError32::InvalidParameter)),
        }
    }
    pub fn report_windows_thread_execution(
        &mut self,
        epoch: u64,
        duration_ns: u64,
    ) -> Result<(), HostError> {
        self.memory()?;
        self.validate_execution_return_epoch(epoch)?;
        let current = self.threads.current_id();
        let thread = self
            .threads
            .threads
            .iter_mut()
            .find(|thread| thread.id == current && thread.state != ThreadState32::Terminated)
            .ok_or(HostError::Thread(ThreadError32::Stale))?;
        if epoch <= thread.duration_epoch {
            return Err(HostError::Thread(ThreadError32::Stale));
        }
        let next = thread
            .user_ns
            .checked_add(duration_ns)
            .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
        thread.user_ns = next;
        thread.duration_epoch = epoch;
        Ok(())
    }
}
