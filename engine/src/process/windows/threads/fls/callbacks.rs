use super::*;
use crate::process::windows::threads::invocation::{
    ThreadInvocation32, ThreadInvocationCompletion32,
};
use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process::windows::threads) fn advance_windows_fls_operation(
        &mut self,
        operation: u64,
    ) -> Result<bool, HostError> {
        loop {
            let step = self
                .threads
                .fls
                .context
                .step(operation)
                .map_err(HostError::Thread)?;
            if step.complete {
                if let Some(index) = self
                    .threads
                    .fls
                    .replies
                    .iter()
                    .position(|reply| reply.operation == operation)
                {
                    let reply = &self.threads.fls.replies[index];
                    let pending = self.checked_pending_call(reply.owner, reply.token)?;
                    if pending.frame != reply.frame {
                        return Err(HostError::Call(crate::process::CallError::InvalidRequest));
                    }
                    let prepared = Self::prepare_completed_call(reply.frame.complete(1))?;
                    let thread = self.current_windows_thread()?;
                    self.publish_windows_thread(thread)?;
                    self.threads.fls.context.commit_step(step);
                    self.threads.fls.replies.remove(index);
                    self.publish_prepared_call(prepared);
                } else {
                    self.threads.fls.context.commit_step(step);
                }
                return Ok(true);
            }
            if step.procedure == 0 {
                self.threads.fls.context.commit_step(step);
                continue;
            }
            self.memory()?
                .check_span(GuestAddress(step.procedure), 1, Access::Execute)
                .map_err(HostError::Memory)?;
            let next = self
                .threads
                .next_invocation
                .checked_add(1)
                .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
            let mut arguments = [0; 16];
            arguments[0] = step.value;
            self.enter_thread_invocation(ThreadInvocation32 {
                id: next,
                target: self.threads.current_id(),
                procedure: GuestAddress(step.procedure),
                arguments,
                count: 1,
                completion: ThreadInvocationCompletion32::Fls { operation },
                timer: None,
                activation: None,
            })?;
            self.threads.next_invocation = next;
            self.threads.fls.context.commit_step(step);
            return Ok(false);
        }
    }
    pub(in crate::process) fn advance_windows_fls_process_exit(
        &mut self,
    ) -> Result<bool, HostError> {
        self.advance_windows_fls_thread_exit()
    }
    pub(in crate::process::windows::threads) fn advance_windows_fls_thread_exit(
        &mut self,
    ) -> Result<bool, HostError> {
        let thread = self.threads.current_id();
        let operation = self
            .threads
            .fls
            .context
            .begin_exit(thread)
            .map_err(HostError::Thread)?;
        let Some(operation) = operation else {
            return Ok(true);
        };
        self.advance_windows_fls_operation(operation)
    }
}
