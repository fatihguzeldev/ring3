use super::TaskReturnSources;
use crate::{
    cpu::x86::tasks::TaskRegister,
    process::{CallError, EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process) fn task_return_configuration_guard(&self) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if self.task_return_sources.is_some()
            || self.generation != 0
            || self.artifact.is_some()
            || self.resident.is_some()
            || self.image_started
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    /// select cached tr, the live configured gdt extent and mapped linear ram as tss-byte sources.
    /// this supplies neither page-fault metadata nor task-switch control/translation authority.
    pub fn configure_task_return_sources(
        &mut self,
        current: TaskRegister,
        gdt_limit: u16,
    ) -> Result<(), HostError> {
        self.task_return_configuration_guard()?;
        self.task_return_sources = Some(TaskReturnSources { current, gdt_limit });
        Ok(())
    }
}

#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
