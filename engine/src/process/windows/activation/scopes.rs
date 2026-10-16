use crate::{
    memory::PageRange,
    process::{EngineInstance, HostError},
    windows::activation::{
        ActivationScope32, PreparedActivationPush32, PreparedActivationScopeExit32,
    },
};

pub(in crate::process) struct PreparedActivationCapture32 {
    handle: u32,
    revision: Option<u64>,
}
impl PreparedActivationCapture32 {
    pub(in crate::process) fn handle(&self) -> u32 {
        self.handle
    }
}
pub(in crate::process) struct PreparedActivationEntry32 {
    push: PreparedActivationPush32,
    retained: u32,
    thread: u32,
}
pub(in crate::process) struct PreparedActivationExit32 {
    native: PreparedActivationScopeExit32,
    ranges: Vec<PageRange>,
}
impl EngineInstance {
    pub(in crate::process) fn prepare_activation_capture(
        &self,
    ) -> Result<PreparedActivationCapture32, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(PreparedActivationCapture32 {
                handle: 0,
                revision: None,
            });
        };
        let handle = runtime.native.current(self.current_windows_thread_id()?);
        self.prepare_activation_capture_handle(handle)
    }
    pub(in crate::process) fn prepare_activation_capture_handle(
        &self,
        handle: u32,
    ) -> Result<PreparedActivationCapture32, HostError> {
        let Some(runtime) = &self.activation else {
            if handle != 0 {
                return Err(HostError::Infrastructure);
            }
            return Ok(PreparedActivationCapture32 {
                handle: 0,
                revision: None,
            });
        };
        if handle != 0 {
            runtime
                .storage(handle)
                .map_err(HostError::Activation)?
                .check(self.memory()?)
                .map_err(HostError::Activation)?;
            runtime
                .native
                .can_retain_hold(handle)
                .map_err(HostError::Activation)?;
        }
        Ok(PreparedActivationCapture32 {
            handle,
            revision: Some(runtime.native.revision()),
        })
    }
    pub(in crate::process) fn commit_activation_capture(
        &mut self,
        prepared: PreparedActivationCapture32,
    ) -> u32 {
        if let Some(revision) = prepared.revision {
            let runtime = self
                .activation
                .as_mut()
                .expect("capture retained the activation runtime");
            assert_eq!(runtime.native.revision(), revision);
            runtime
                .native
                .retain_hold(prepared.handle)
                .expect("activation hold was preflighted before enqueue");
        }
        prepared.handle
    }
    pub(in crate::process) fn prepare_activation_entry(
        &self,
        retained: Option<u32>,
    ) -> Result<Option<PreparedActivationEntry32>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        let thread = self.current_windows_thread_id()?;
        let handle = retained.unwrap_or_else(|| runtime.native.current(thread));
        if handle != 0 {
            runtime
                .storage(handle)
                .map_err(HostError::Activation)?
                .check(self.memory()?)
                .map_err(HostError::Activation)?;
        }
        let push = runtime
            .native
            .prepare_activate(thread, handle)
            .map_err(HostError::Activation)?;
        Ok(Some(PreparedActivationEntry32 {
            push,
            retained: retained.unwrap_or(0),
            thread,
        }))
    }
    pub(in crate::process) fn prepare_activation_entry_borrowed(
        &self,
        handle: u32,
    ) -> Result<Option<PreparedActivationEntry32>, HostError> {
        let mut prepared = self.prepare_activation_entry(Some(handle))?;
        if let Some(prepared) = &mut prepared {
            prepared.retained = 0;
        }
        Ok(prepared)
    }
    pub(in crate::process) fn commit_activation_entry(
        &mut self,
        prepared: Option<PreparedActivationEntry32>,
    ) -> Option<ActivationScope32> {
        prepared.map(|prepared| {
            let cookie = self
                .activation
                .as_mut()
                .unwrap()
                .native
                .commit_activate(prepared.push);
            ActivationScope32::new(prepared.thread, cookie, prepared.retained)
        })
    }
    pub(in crate::process) fn prepare_activation_scope_exit(
        &self,
        scope: Option<ActivationScope32>,
    ) -> Result<Option<PreparedActivationExit32>, HostError> {
        let Some(scope) = scope else { return Ok(None) };
        let runtime = self.activation.as_ref().ok_or(HostError::Infrastructure)?;
        let native = runtime
            .native
            .prepare_scope_exit(scope)
            .map_err(HostError::Activation)?;
        let mut ranges = runtime
            .disposal_ranges(self.memory()?, native.disposed())
            .map_err(HostError::Activation)?;
        if !ranges.is_empty() {
            ranges.sort_unstable_by_key(|range| range.first);
            for pair in ranges.windows(2) {
                if pair[0].first + pair[0].count > pair[1].first {
                    return Err(HostError::Infrastructure);
                }
            }
            self.memory()?
                .ensure_transaction_versions(1)
                .map_err(HostError::Memory)?;
        }
        Ok(Some(PreparedActivationExit32 { native, ranges }))
    }
    pub(in crate::process) fn commit_activation_scope_exit(
        &mut self,
        prepared: Option<PreparedActivationExit32>,
    ) {
        let Some(prepared) = prepared else { return };
        if !prepared.ranges.is_empty() {
            self.memory
                .as_mut()
                .unwrap()
                .unmap_ranges_with_word_writes(&prepared.ranges, &[])
                .expect("activation callback mappings were preflighted before completion");
        }
        let runtime = self.activation.as_mut().unwrap();
        let disposed = runtime.native.commit_scope_exit(prepared.native);
        runtime.retire_storage(&disposed);
    }
}
