use crate::{
    abi::{
        arena::{STATE_OFFSET, TRANSFER_OFFSET},
        tasks::{TASK_CONTEXT_SIZE, TaskContextRecord32, encode_task_context},
        x86::{STATE_SIZE, decode_state, decode_state_metadata},
    },
    cpu::x86::tasks::TaskIncomingContext32,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    /// serialize committed raw task context without granting execution authority or accepting host writes.
    pub fn read_task_context(&mut self) -> Result<(), HostError> {
        self.memory()?;
        let record = match self.task_context_snapshot()? {
            Some(context) => TaskContextRecord32::Loaded {
                revision: self.task_execution_revision,
                context,
                ss_debug_shadow: decode_state_metadata(
                    &self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
                )
                .map_err(|_| HostError::Infrastructure)?
                .ss_debug_shadow,
            },
            None => TaskContextRecord32::Absent,
        };
        let mut bytes = [0; TASK_CONTEXT_SIZE];
        encode_task_context(record, &mut bytes).map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE]
            .copy_from_slice(&bytes);
        Ok(())
    }

    pub(in crate::process) fn task_context_snapshot(
        &self,
    ) -> Result<Option<TaskIncomingContext32>, HostError> {
        self.memory()?;
        let Some(context) = self.task_incoming else {
            return Ok(None);
        };
        let state = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        // State32 in the arena is the live GPR/flags owner; the private object owns raw/cache progress.
        let mut context = context.with_state(state);
        if context.validated_segments().is_some() {
            context = context.with_segments(self.segment_context()?);
        }
        Ok(Some(context))
    }
}

#[cfg(test)]
mod tests;
