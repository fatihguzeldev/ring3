use super::*;
use crate::{
    abi::x86::{STATE_SIZE, decode_state},
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::{CallbackFrame32, winmm::timers::MultimediaTimerCallback32},
};

impl EngineInstance {
    pub(in crate::process) fn ensure_multimedia_timer_worker(&mut self) -> Result<u32, HostError> {
        if let Some((id, handle)) = self.threads.multimedia_worker {
            if self.threads.info(id).map_err(HostError::Thread)?.state != ThreadState32::Terminated
            {
                return Ok(id);
            }
            self.synchronization
                .as_mut()
                .ok_or(HostError::Infrastructure)?
                .context_mut()
                .close_handle(handle)
                .map_err(HostError::Synchronization)?;
            self.threads.multimedia_worker = None;
        }
        let idle = self
            .module_gates()?
            .iter()
            .find(|gate| gate.id == THREAD_RETURN_ID)
            .map(|gate| gate.entry.0)
            .ok_or(HostError::Thread(ThreadError32::Unsupported))?;
        let id = self.threads.next_id;
        let mut active = Vec::new();
        active
            .try_reserve_exact(1)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        let handle = self.create_windows_thread_at_gate(&[0, 0, idle, 0, 0, 0], idle)?;
        let thread = self
            .threads
            .threads
            .iter_mut()
            .find(|thread| thread.id == id)
            .expect("thread creation published the prepared identity");
        thread.state = ThreadState32::Waiting;
        thread.active_invocations = active;
        self.threads.multimedia_worker = Some((id, handle));
        Ok(id)
    }

    pub(in crate::process) fn prepare_multimedia_timer_callback(
        &self,
        worker: u32,
        procedure: GuestAddress,
        arguments: [u32; 5],
        identity: MultimediaTimerCallback32,
        activation: u32,
        owned: bool,
    ) -> Result<PreparedThreadInvocation32, HostError> {
        if !self
            .threads
            .multimedia_worker
            .is_some_and(|(id, _)| id == worker)
        {
            return Err(HostError::Thread(ThreadError32::InvalidHandle));
        }
        let invocation = self.prepare_thread_invocation_with_activation(
            worker,
            procedure,
            &arguments,
            ThreadInvocationCompletion32::MultimediaTimer(identity),
            Some(activation),
            owned,
        )?;
        let thread = self
            .threads
            .threads
            .iter()
            .find(|thread| thread.id == worker)
            .ok_or(HostError::Thread(ThreadError32::InvalidHandle))?;
        let idle = thread
            .active_invocations
            .first()
            .map(|active| &active.saved)
            .or(thread.saved.as_ref());
        let arena = idle.map_or(self.arena(), |saved| &saved.arena);
        let state = decode_state(&arena[..STATE_SIZE]).map_err(|_| HostError::Infrastructure)?;
        let return_pc = self
            .module_gates()?
            .iter()
            .find(|gate| gate.id == INVOCATION_RETURN_ID)
            .map(|gate| gate.entry.0)
            .ok_or(HostError::Thread(ThreadError32::Unsupported))?;
        let frame = CallbackFrame32::prepare(state, procedure.0, return_pc, &arguments)
            .map_err(|_| HostError::Thread(ThreadError32::InvalidParameter))?;
        self.memory()?
            .ensure_transaction_versions(1)
            .map_err(HostError::Memory)?;
        for write in frame.writes() {
            self.memory()?
                .check_span(write.address, 4, Access::Write)
                .map_err(HostError::Memory)?;
        }
        Ok(invocation)
    }

    pub(super) fn has_pending_multimedia_callback(&self, thread: u32) -> bool {
        self.threads.invocations.iter().any(|invocation| {
            invocation.target == thread
                && matches!(
                    invocation.completion,
                    ThreadInvocationCompletion32::MultimediaTimer(_)
                )
        })
    }

    pub(in crate::process) fn multimedia_worker_handle(&self) -> Option<u32> {
        self.threads.multimedia_worker.map(|(_, handle)| handle)
    }
}
