use super::decoding;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::synchronization::{
        SynchronizationContext32, SynchronizationError32 as Error, WaitOutcome32, WaitResult32,
    },
};

impl EngineInstance {
    pub(super) fn with_windows_wait_objects(
        &mut self,
        handles: &[u32],
        wait: impl FnOnce(&mut SynchronizationContext32) -> Result<WaitOutcome32, Error>,
    ) -> Result<WaitOutcome32, HostError> {
        let process = self.current_windows_process_id();
        self.synchronization_mut()?
            .with_current_wait_objects(process, handles, wait)
            .map_err(HostError::Synchronization)
    }

    pub(in crate::process) fn validate_windows_process_handle(
        &self,
        handle: u32,
        access: u32,
    ) -> Result<(), Error> {
        if handle == u32::MAX {
            return Ok(());
        }
        let process = self
            .synchronization
            .as_ref()
            .ok_or(Error::InvalidHandle)?
            .context()
            .validate_process_access(handle, access)?;
        if process == self.current_windows_process_id() {
            Ok(())
        } else {
            Err(Error::InvalidHandle)
        }
    }
}

impl EngineInstance {
    pub(super) fn duplicate_windows_handle_call(
        &mut self,
        args: &[u32],
    ) -> Result<WaitOutcome32, HostError> {
        self.validate_windows_process_handle(args[0], 0x40)
            .map_err(HostError::Synchronization)?;
        let close = if args[6] & 1 != 0 && !matches!(args[1], u32::MAX | 0xffff_fffe) {
            Some(
                self.synchronization()?
                    .prepare_duplicate(args[1], 0, false, true, true, false)
                    .map_err(HostError::Synchronization)?,
            )
        } else {
            None
        };
        let result = self.duplicate_windows_handle_inner(args);
        if matches!(result, Err(HostError::Synchronization(_)))
            && let Some(close) = close
        {
            self.synchronization_mut()?.commit_duplicate(close);
        }
        result
    }

    fn duplicate_windows_handle_inner(&mut self, args: &[u32]) -> Result<WaitOutcome32, HostError> {
        if !(args[2] == 0 && args[6] & 1 != 0) {
            self.validate_windows_process_handle(args[2], 0x40)
                .map_err(HostError::Synchronization)?;
        }
        if args[6] & !3 != 0 {
            return Err(HostError::Synchronization(Error::InvalidWait));
        }
        let target = args[2] != 0;
        if target {
            decoding::check_output(self.memory()?, args[3])?;
        }
        if matches!(args[1], u32::MAX | 0xffff_fffe) {
            if !target {
                return Ok(WaitOutcome32::Ready(WaitResult32 {
                    value: 1,
                    last_error: None,
                }));
            }
            let access = if args[6] & 2 != 0 {
                0x001f_ffff
            } else {
                args[4]
            };
            let prepared = if args[1] == u32::MAX {
                self.synchronization()?.prepare_process_handle(
                    self.current_windows_process_id(),
                    access,
                    args[5] != 0,
                )
            } else {
                self.synchronization()?.prepare_open_thread(
                    self.current_windows_thread_id()?,
                    access,
                    args[5] != 0,
                    false,
                )
            }
            .map_err(HostError::Synchronization)?;
            if args[3] != 0 {
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[3]), &prepared.handle().to_le_bytes())
                    .map_err(HostError::Memory)?;
            }
            self.synchronization_mut()?.commit_object_handle(prepared);
            return Ok(WaitOutcome32::Ready(WaitResult32 {
                value: 1,
                last_error: None,
            }));
        }
        let prepared = self.synchronization()?.prepare_duplicate(
            args[1],
            args[4],
            args[5] != 0,
            args[6] & 2 != 0,
            args[6] & 1 != 0,
            target,
        );
        let prepared = prepared.map_err(HostError::Synchronization)?;
        if target && args[3] != 0 {
            self.memory
                .as_mut()
                .ok_or(HostError::Closed)?
                .write(
                    GuestAddress(args[3]),
                    &prepared.handle().unwrap().to_le_bytes(),
                )
                .map_err(HostError::Memory)?;
        }
        self.synchronization_mut()?.commit_duplicate(prepared);
        Ok(WaitOutcome32::Ready(WaitResult32 {
            value: 1,
            last_error: None,
        }))
    }
}
