use crate::process::{
    EngineInstance, HostError,
    calls::call::PendingOwner,
    windows::ole32::marshaling::{ExecutionApartment32, Remote32},
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn stream_copy_invocation(
        &self,
        sender: u32,
        continuation: u64,
    ) -> Option<Result<ExecutionApartment32, HostError>> {
        let index = self
            .ole32
            .as_ref()?
            .streams
            .copies
            .iter()
            .position(|call| {
                call.execution.thread == sender
                    && call.continuation == continuation
                    && call.remote == Remote32::Queued
            })?;
        Some(
            self.check_stream_copy(index)
                .map(|_| self.ole32.as_ref().unwrap().streams.copies[index].execution),
        )
    }
    pub(in crate::process::windows::ole32) fn stream_copy_queued(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .streams
                .copies
                .iter()
                .any(|call| call.execution.thread == thread && call.remote == Remote32::Queued)
        })
    }
    pub(in crate::process::windows::ole32) fn complete_stream_copy_invocation(
        &mut self,
        sender: u32,
        continuation: u64,
        result: u32,
    ) -> bool {
        let Some(call) = self.ole32.as_mut().and_then(|runtime| {
            runtime.streams.copies.iter_mut().find(|call| {
                call.execution.thread == sender
                    && call.continuation == continuation
                    && call.remote == Remote32::Queued
            })
        }) else {
            return false;
        };
        call.remote = Remote32::Ready(result);
        self.wake_com_apartment_sender(sender);
        true
    }
    pub(in crate::process::windows::ole32) fn stream_copy_ready(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .streams
                .copies
                .iter()
                .any(|call| call.execution.thread == thread && call.remote != Remote32::Queued)
        })
    }
    pub(in crate::process::windows::ole32) fn resume_stream_copy(
        &mut self,
    ) -> Result<bool, HostError> {
        let Some(pending) = &self.pending_call else {
            return Ok(false);
        };
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .streams
                .copies
                .iter()
                .position(|call| call.owner == pending.owner && call.token == pending.token)
        }) else {
            return Ok(false);
        };
        let call = &self.ole32.as_ref().unwrap().streams.copies[index];
        if self.checked_pending_call(call.owner, call.token)?.frame != call.frame
            || call.execution.thread != self.current_windows_thread_id()?
        {
            return Err(HostError::InvalidRequest);
        }
        self.advance_stream_copy(index)?;
        Ok(true)
    }
    pub(in crate::process::windows::ole32) fn stream_copy_owns_token(&self, token: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .streams
                .copies
                .iter()
                .any(|call| call.token == token)
        })
    }
    pub(in crate::process::windows::ole32) fn stream_copy_pending_owners(
        &self,
    ) -> impl Iterator<Item = PendingOwner> + '_ {
        self.ole32
            .iter()
            .flat_map(|runtime| runtime.streams.copies.iter().map(|call| call.owner))
    }
}
