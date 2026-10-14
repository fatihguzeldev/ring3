use crate::{
    abi::{
        arena::{HELPER_OFFSET, STATE_OFFSET},
        memory_helper::HELPER_SIZE,
        tasks::{TaskTransitionReply32, encode_task_transition_result},
        x86::{STATE_SIZE, decode_state},
    },
    cpu::x86::tasks::TaskSave,
    process::{EngineInstance, HostError, StoreCompletion, TaskReturnTransition32},
};

impl EngineInstance {
    pub fn cpu_task_return(&mut self, next_pc: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.cpu_task_return_bound(None, next_pc)
    }

    pub fn cpu_task_return_resident(
        &mut self,
        key: u64,
        id: u64,
        next_pc: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.cpu_task_return_bound(Some(id), next_pc)
    }

    fn cpu_task_return_bound(
        &mut self,
        resident: Option<u64>,
        next_pc: u32,
    ) -> Result<StoreCompletion, HostError> {
        let state = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let material = match resident {
            Some(id) => self
                .resident
                .as_ref()
                .ok_or(HostError::InvalidArtifact)?
                .get_raw(self.memory()?, id)
                .map_err(HostError::Resident)?,
            None => self.artifact.as_ref().ok_or(HostError::InvalidArtifact)?,
        };
        if material.task_return_next_pc(state.eip) != Some(next_pc) {
            return Err(HostError::InvalidRequest);
        }
        let selectors = match self.task_context_snapshot()? {
            Some(context) => context.selectors(),
            None => self.segment_context()?.state.selectors,
        };
        let outgoing = TaskSave {
            next_eip: next_pc,
            flags: state.eflags,
            general: state.registers,
            selectors,
        };
        let result = match resident {
            Some(id) => self.task_return_transition_resident(self.key, id, outgoing)?,
            None => self.task_return_transition(outgoing)?,
        };
        let (reply, currentness) = match result {
            TaskReturnTransition32::Committed(committed) => (
                TaskTransitionReply32::Committed {
                    fault: committed.context.fault(),
                    task_trap: committed.context.task_trap(),
                    code_invalidated: committed.currentness == Ok(StoreCompletion::CodeInvalidated),
                },
                committed.currentness,
            ),
            TaskReturnTransition32::PreCommitFault(fault) => (
                TaskTransitionReply32::Fault(fault),
                Ok(StoreCompletion::Complete),
            ),
            TaskReturnTransition32::NeedsAuthority(requirement) => (
                TaskTransitionReply32::Authority(requirement),
                Ok(StoreCompletion::Complete),
            ),
            TaskReturnTransition32::NeedsPagingAuthority { fault, length } => (
                TaskTransitionReply32::Memory { fault, length },
                Ok(StoreCompletion::Complete),
            ),
            TaskReturnTransition32::Infrastructure(error) => (
                TaskTransitionReply32::Infrastructure(error),
                Ok(StoreCompletion::Complete),
            ),
        };
        let mut packet = [0; HELPER_SIZE];
        encode_task_transition_result(reply, &mut packet).map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet);
        currentness
    }
}
