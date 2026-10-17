use super::context::{Action32, Stage32};
use crate::{
    memory::PageRange,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::CallFrame32,
};

impl EngineInstance {
    pub(in crate::process) fn check_automation_callback(
        &self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        continuation: u64,
    ) -> Result<(), HostError> {
        let thread = self.current_windows_thread_id()?;
        let work = self
            .automation
            .references
            .iter()
            .find(|work| {
                work.owner == owner
                    && work.token == token
                    && work.frame == frame
                    && work.continuation == continuation
                    && work.thread == thread
            })
            .ok_or(HostError::InvalidRequest)?;
        let Stage32::Calling { action, call } = work.stage else {
            return Err(HostError::Call(CallError::StateChanged));
        };
        if !matches!(action, Action32::Compensate | Action32::ReleaseTemporary) {
            if let Some(output) = &work.output {
                output.check(self.memory()?)?;
            }
            if let Some(loan) = &work.array {
                self.check_automation_array_loan(loan)?;
            }
            self.check_retained_variant_allocations(&work.plan)?;
        }
        work.objects.check(self.memory()?, call.object)?;
        call.check(self.memory()?)?;
        if matches!(action, Action32::Query(_) | Action32::Invoke) {
            work.scratch
                .ok_or(HostError::Call(CallError::StateChanged))?
                .check(self.memory()?)
                .map_err(HostError::Memory)?;
        }
        Ok(())
    }
    pub(in crate::process) fn automation_pending_owners(
        &self,
    ) -> impl Iterator<Item = PendingOwner> + '_ {
        self.automation.references.iter().map(|work| work.owner)
    }
    pub(in crate::process) fn automation_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self
                .automation
                .references
                .iter()
                .any(|work| work.token == token)
    }
    pub(in crate::process) fn automation_thread_scratch_ranges(
        &self,
        thread: u32,
    ) -> impl Iterator<Item = PageRange> + '_ {
        self.automation
            .references
            .iter()
            .filter(move |work| work.thread == thread)
            .filter_map(|work| work.scratch.map(|scratch| scratch.range()))
    }
    pub(in crate::process) fn automation_process_scratch_ranges(
        &self,
    ) -> impl Iterator<Item = PageRange> + '_ {
        self.automation
            .references
            .iter()
            .filter_map(|work| work.scratch.map(|scratch| scratch.range()))
    }
    pub(in crate::process) fn check_automation_thread_scratch(
        &self,
        thread: u32,
    ) -> Result<(), HostError> {
        for work in self
            .automation
            .references
            .iter()
            .filter(|work| work.thread == thread)
        {
            if let Some(scratch) = work.scratch {
                scratch.check(self.memory()?).map_err(HostError::Memory)?;
            }
        }
        Ok(())
    }
    pub(in crate::process) fn check_automation_process_scratch(&self) -> Result<(), HostError> {
        for work in &self.automation.references {
            if let Some(scratch) = work.scratch {
                scratch.check(self.memory()?).map_err(HostError::Memory)?;
            }
        }
        Ok(())
    }
    pub(in crate::process) fn automation_retire_thread(&mut self, thread: u32) {
        self.automation
            .references
            .retain(|work| work.thread != thread);
    }
}
