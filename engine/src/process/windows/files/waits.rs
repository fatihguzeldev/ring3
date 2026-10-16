use super::*;
use crate::{
    process::{EngineInstance, HostError},
    windows::synchronization::{SynchronizationContext32, SynchronizationError32},
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn with_file_event<T>(
        &mut self,
        open: OpenFileId,
        sync: &mut SynchronizationContext32,
        operation: impl FnOnce(&mut Self, &mut SynchronizationContext32, u32) -> Result<T, FileError32>,
    ) -> Result<T, FileError32> {
        self.with_io_event(
            crate::windows::files::completion::CompletionTarget32::File(open),
            sync,
            operation,
        )
    }
    pub(in crate::process::windows::files) fn with_io_event<T>(
        &mut self,
        target: crate::windows::files::completion::CompletionTarget32,
        sync: &mut SynchronizationContext32,
        operation: impl FnOnce(&mut Self, &mut SynchronizationContext32, u32) -> Result<T, FileError32>,
    ) -> Result<T, FileError32> {
        let existing = self.events.iter().any(|(id, _)| *id == target);
        if !existing {
            sync.revision()
                .checked_add(2)
                .ok_or(FileError32::Exhausted)?;
        }
        let event = self.io_event(target, sync)?;
        let next_operation = self.asynchronous.next;
        let result = operation(self, sync, event);
        if result.is_err() && !existing && self.asynchronous.next == next_operation {
            sync.close_handle(event)
                .expect("new private I/O event rollback revision was preflighted");
            self.events.retain(|(id, _)| *id != target);
        }
        result
    }
    pub(super) fn validate_io_description(
        &self,
        target: crate::windows::files::completion::CompletionTarget32,
    ) -> Result<(), FileError32> {
        match target {
            crate::windows::files::completion::CompletionTarget32::Null(id) => {
                self.null_devices.overlapped(id)?;
            }
            crate::windows::files::completion::CompletionTarget32::File(open) => {
                self.vfs.open_info(open)?;
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn file_event(
        &mut self,
        open: OpenFileId,
        sync: &mut SynchronizationContext32,
    ) -> Result<u32, FileError32> {
        self.io_event(
            crate::windows::files::completion::CompletionTarget32::File(open),
            sync,
        )
    }
    pub(super) fn io_event(
        &mut self,
        target: crate::windows::files::completion::CompletionTarget32,
        sync: &mut SynchronizationContext32,
    ) -> Result<u32, FileError32> {
        if let Some((_, handle)) = self.events.iter().find(|(id, _)| *id == target) {
            return Ok(*handle);
        }
        self.validate_io_description(target)?;
        self.events
            .try_reserve(1)
            .map_err(|_| FileError32::Allocation)?;
        let (event, _) = sync
            .create_event(
                true,
                self.asynchronous.is_description_signaled(target),
                false,
                None,
            )
            .map_err(sync_error)?;
        self.events.push((target, event));
        Ok(event)
    }
}
impl EngineInstance {
    pub(in crate::process) fn with_kernel_wait_handles<T>(
        &mut self,
        handles: &mut [u32],
        operation: impl FnOnce(&mut Self, &[u32]) -> Result<T, HostError>,
    ) -> Result<T, HostError> {
        let previous = self.files.as_ref().map_or(0, |files| files.events.len());
        let result = self
            .resolve_kernel_wait_handles(handles)
            .and_then(|()| operation(self, handles));
        if result.is_err()
            && let Some(files) = self.files.as_mut()
            && files.events.len() > previous
        {
            let sync = self.synchronization.as_mut().unwrap().context_mut();
            for &(_, event) in &files.events[previous..] {
                sync.close_handle(event)
                    .expect("private wait event rollback revisions were preflighted");
            }
            files.events.truncate(previous);
        }
        result
    }
    pub(in crate::process) fn file_mapping_name_conflict(
        &self,
        name: &[u16],
    ) -> Result<bool, SynchronizationError32> {
        self.files.as_ref().map_or(Ok(false), |files| {
            files
                .mappings
                .has_named_mapping(name)
                .map_err(|error| match error {
                    FileError32::Unsupported => SynchronizationError32::Unsupported,
                    _ => SynchronizationError32::InvalidName,
                })
        })
    }
    pub(in crate::process) fn resolve_kernel_wait_handles(
        &mut self,
        handles: &mut [u32],
    ) -> Result<(), HostError> {
        if handles.len() > 64 {
            return Err(HostError::Synchronization(
                SynchronizationError32::InvalidWait,
            ));
        }
        for (index, handle) in handles.iter().enumerate() {
            if handles[..index].contains(handle) {
                return Err(HostError::Synchronization(
                    SynchronizationError32::InvalidWait,
                ));
            }
        }
        let sync = self
            .synchronization
            .as_ref()
            .ok_or(HostError::Synchronization(
                SynchronizationError32::Unsupported,
            ))?
            .context();
        let mut missing = [None; 64];
        let mut count = 0;
        for &handle in handles.iter() {
            if matches!(handle, u32::MAX | 0xffff_fffe) {
                continue;
            }
            if handle < 0x8000_0000 {
                sync.validate_wait_handle(handle)
                    .map_err(HostError::Synchronization)?;
                continue;
            }
            let files = self.files.as_ref().ok_or(HostError::Synchronization(
                SynchronizationError32::InvalidHandle,
            ))?;
            let target = files
                .handles
                .io_target(handle)
                .map_err(|_| HostError::Synchronization(SynchronizationError32::InvalidHandle))?;
            if files
                .handles
                .access(handle)
                .map_err(|error| HostError::Synchronization(file_wait_error(error)))?
                & 0x0010_0000
                == 0
            {
                return Err(HostError::Synchronization(
                    SynchronizationError32::AccessDenied,
                ));
            }
            files
                .validate_io_description(target)
                .map_err(|error| HostError::Synchronization(file_wait_error(error)))?;
            if !files.events.iter().any(|(id, _)| *id == target)
                && !missing[..count].contains(&Some(target))
            {
                missing[count] = Some(target);
                count += 1;
            }
        }
        sync.ensure_unnamed_event_capacity(count)
            .map_err(HostError::Synchronization)?;
        sync.revision()
            .checked_add(count as u64 * 2 + 1)
            .ok_or(HostError::Synchronization(
                SynchronizationError32::Exhausted,
            ))?;
        if let Some(files) = self.files.as_mut() {
            files
                .events
                .try_reserve(count)
                .map_err(|_| HostError::Synchronization(SynchronizationError32::Allocation))?;
        }
        for handle in handles {
            if *handle < 0x8000_0000 || matches!(*handle, u32::MAX | 0xffff_fffe) {
                continue;
            }
            let files = self.files.as_mut().unwrap();
            let target = files.handles.io_target(*handle).unwrap();
            let sync = self.synchronization.as_mut().unwrap().context_mut();
            *handle = files
                .io_event(target, sync)
                .map_err(|error| HostError::Synchronization(file_wait_error(error)))?;
        }
        Ok(())
    }
}
fn sync_error(error: SynchronizationError32) -> FileError32 {
    match error {
        SynchronizationError32::Capacity => FileError32::Capacity,
        SynchronizationError32::Allocation => FileError32::Allocation,
        SynchronizationError32::Exhausted => FileError32::Exhausted,
        _ => FileError32::InvalidHandle,
    }
}

fn file_wait_error(error: FileError32) -> SynchronizationError32 {
    match error {
        FileError32::Allocation => SynchronizationError32::Allocation,
        FileError32::Capacity => SynchronizationError32::Capacity,
        FileError32::Exhausted => SynchronizationError32::Exhausted,
        FileError32::AccessDenied => SynchronizationError32::AccessDenied,
        FileError32::Unsupported => SynchronizationError32::Unsupported,
        _ => SynchronizationError32::InvalidHandle,
    }
}

pub(in crate::process) struct PreparedFileEventCleanup32 {
    opens: Vec<OpenFileId>,
    events: Vec<(crate::windows::files::completion::CompletionTarget32, u32)>,
    close: Option<crate::windows::synchronization::PreparedCloseHandles32>,
}
impl FileRuntime32 {
    fn prepare_file_event_cleanup(
        &self,
        sync: Option<&SynchronizationContext32>,
        closes: &[(OpenFileId, u32)],
    ) -> Result<PreparedFileEventCleanup32, FileError32> {
        let mut opens = Vec::new();
        let mut events = Vec::new();
        let mut handles = Vec::new();
        opens
            .try_reserve_exact(closes.len())
            .map_err(|_| FileError32::Allocation)?;
        events
            .try_reserve_exact(closes.len())
            .map_err(|_| FileError32::Allocation)?;
        handles
            .try_reserve_exact(closes.len())
            .map_err(|_| FileError32::Allocation)?;
        for &(open, count) in closes {
            let numeric = count.checked_sub(1).ok_or(FileError32::InvalidParameter)? as usize;
            if self.handles.file_count(open) != numeric || opens.contains(&open) {
                continue;
            }
            opens.push(open);
            if let Some(&(id, event)) = self.events.iter().find(|(id, _)| {
                *id == crate::windows::files::completion::CompletionTarget32::File(open)
            }) {
                events.push((id, event));
                handles.push(event);
            }
        }
        let close = if handles.is_empty() {
            None
        } else {
            Some(
                sync.ok_or(FileError32::Unsupported)?
                    .prepare_close_handles(&handles)
                    .map_err(sync_error)?,
            )
        };
        Ok(PreparedFileEventCleanup32 {
            opens,
            events,
            close,
        })
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_file_callback_cleanup(
        &self,
        continuation: u64,
    ) -> Result<PreparedFileEventCleanup32, HostError> {
        let files = self.files.as_ref().ok_or(HostError::InvalidRequest)?;
        let closes = files
            .pending_copy_closes(continuation)
            .map_err(HostError::File)?;
        files
            .prepare_file_event_cleanup(
                self.synchronization
                    .as_ref()
                    .map(|runtime| runtime.context()),
                &closes,
            )
            .map_err(HostError::File)
    }
    pub(in crate::process) fn commit_file_callback_cleanup(
        &mut self,
        prepared: PreparedFileEventCleanup32,
    ) {
        let files = self.files.as_mut().expect("prepared file callback runtime");
        for open in prepared.opens {
            assert_eq!(files.handles.file_count(open), 0);
            files.asynchronous.cancel_open(open);
            files.release_file_locks(open);
        }
        if let Some(close) = prepared.close {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_close_handles(close);
        }
        files
            .events
            .retain(|event| !prepared.events.contains(event));
    }
}
