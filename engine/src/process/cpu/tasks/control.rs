use crate::{
    cpu::x86::{
        UserSystemRegisterBank32,
        tasks::{TaskControlState32, TaskIncomingContext32},
    },
    process::{CallError, EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct TaskControlAuthority {
    pub(super) state: TaskControlState32,
    pub(super) memory_identity: u64,
}

impl EngineInstance {
    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn configure_task_controls_request(&mut self) -> Result<(), HostError> {
        use crate::abi::{
            arena::TRANSFER_OFFSET,
            tasks::{TASK_CONTROLS_SIZE, decode_task_controls},
        };
        self.memory()?;
        let controls = decode_task_controls(
            &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTROLS_SIZE],
        )
        .map_err(|_| HostError::InvalidRequest)?;
        self.configure_task_controls(controls)
    }

    pub fn read_task_controls(&mut self) -> Result<(), HostError> {
        use crate::abi::{
            arena::TRANSFER_OFFSET,
            tasks::{TASK_CONTROLS_SIZE, encode_task_controls},
        };
        let controls = self
            .task_control_state()?
            .ok_or(HostError::InvalidRequest)?;
        let mut bytes = [0; TASK_CONTROLS_SIZE];
        encode_task_controls(controls, &mut bytes).map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTROLS_SIZE]
            .copy_from_slice(&bytes);
        Ok(())
    }

    pub fn configure_task_controls(&mut self, state: TaskControlState32) -> Result<(), HostError> {
        let memory_identity = self.memory()?.identity();
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if self.task_return_sources.is_none()
            || self.task_controls.is_some()
            || self.generation != 0
            || self.artifact.is_some()
            || self.resident.is_some()
            || self.image_started
        {
            return Err(HostError::InvalidRequest);
        }
        self.task_controls = Some(TaskControlAuthority {
            state,
            memory_identity,
        });
        self.task_execution_revision = 1;
        Ok(())
    }

    pub fn task_control_state(&self) -> Result<Option<TaskControlState32>, HostError> {
        self.memory()?;
        Ok(self.task_controls.map(|controls| controls.state))
    }

    pub fn task_incoming_context(&self) -> Result<Option<TaskIncomingContext32>, HostError> {
        self.memory()?;
        self.task_context_snapshot()
    }

    pub(in crate::process) fn task_coprocessor_cr0(&self) -> Option<u32> {
        self.task_controls.map(|controls| controls.state.cr0())
    }

    pub(in crate::process) fn task_execution_revision(&self) -> Option<u64> {
        self.task_controls.map(|_| self.task_execution_revision)
    }

    pub(in crate::process) fn task_system_register_bank(&self) -> Option<UserSystemRegisterBank32> {
        let Some(controls) = self.task_controls else {
            return self.system_register_bank;
        };
        let mut bank = self.system_register_bank.unwrap_or_default();
        bank.tr = self
            .task_return_sources
            .map(|sources| sources.current.selector());
        bank.ldtr = Some(controls.state.ldtr());
        bank.cr0_low16 = Some(controls.state.cr0() as u16);
        Some(bank)
    }
}

#[cfg(test)]
#[path = "control/tests.rs"]
mod tests;
