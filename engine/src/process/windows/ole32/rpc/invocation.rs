use super::context::RpcPhase32;
use crate::{
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::ole32::marshaling::{ExecutionApartment32, Remote32},
    },
    windows::ole32::ApartmentId32,
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn rpc_invocation_execution(
        &self,
        sender: u32,
        continuation: u64,
    ) -> Option<Result<ExecutionApartment32, HostError>> {
        if let Some(execution) = self.rpc_rundown_execution(sender, continuation) {
            return Some(Ok(execution));
        }
        let call = self.ole32.as_ref()?.rpc.calls.iter().find(|call| {
            call.sender == sender
                && call.continuation == continuation
                && call.remote == Remote32::Queued
        })?;
        Some(if let Some(scratch) = call.scratch {
            scratch
                .check(self.memory().ok()?)
                .map(|_| call.execution)
                .map_err(HostError::Memory)
        } else {
            Ok(call.execution)
        })
    }
    pub(in crate::process::windows::ole32) fn has_rpc_invocation(&self, thread: u32) -> bool {
        if self.ole32.as_ref().is_some_and(|runtime| {
            runtime.rpc.brokers.iter().flatten().any(|broker| {
                broker.rundown.is_some_and(|rundown| {
                    rundown.execution.thread == thread && rundown.remote == Remote32::Queued
                })
            })
        }) {
            return true;
        }
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .rpc
                .calls
                .iter()
                .any(|call| call.execution.thread == thread && call.remote == Remote32::Queued)
        })
    }
    pub(in crate::process::windows::ole32) fn rpc_same_thread_invocation(
        &self,
        thread: u32,
    ) -> bool {
        if self.ole32.as_ref().is_some_and(|runtime| {
            runtime.rpc.brokers.iter().flatten().any(|broker| {
                broker.rundown.is_some_and(|rundown| {
                    rundown.sender == thread
                        && rundown.execution.thread == thread
                        && rundown.remote == Remote32::Queued
                })
            })
        }) {
            return true;
        }
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.rpc.calls.iter().any(|call| {
                call.sender == thread
                    && call.execution.thread == thread
                    && call.remote == Remote32::Queued
                    && (call.execution.apartment == ApartmentId32::Neutral
                        || runtime.apartments.identity(thread) == Ok(call.execution.apartment))
            })
        })
    }
    pub(in crate::process::windows::ole32) fn complete_rpc_invocation(
        &mut self,
        sender: u32,
        continuation: u64,
        result: u32,
    ) -> bool {
        if self.complete_rpc_rundown(sender, continuation) {
            return true;
        }
        let Some(call) = self.ole32.as_mut().and_then(|runtime| {
            runtime.rpc.calls.iter_mut().find(|call| {
                call.sender == sender
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
    pub(in crate::process::windows::ole32) fn rpc_work_ready(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.rpc.calls.iter().any(|call| {
                !call.abandoned
                    && call.sender == thread
                    && call.remote != Remote32::Queued
                    && (call.phase != RpcPhase32::Transfer
                        || self
                            .com_transfer_result(
                                super::super::marshaling::transfer::TransferParent32::RpcQuery(
                                    call.continuation,
                                ),
                            )
                            .is_some())
            })
        })
    }
    pub(in crate::process::windows::ole32) fn rpc_pending_owners(
        &self,
    ) -> impl Iterator<Item = PendingOwner> + '_ {
        self.ole32
            .iter()
            .flat_map(|runtime| runtime.rpc.calls.iter().map(|call| call.owner))
    }
    pub(in crate::process::windows::ole32) fn rpc_owns_token(&self, token: u32) -> bool {
        self.ole32
            .as_ref()
            .is_some_and(|runtime| runtime.rpc.calls.iter().any(|call| call.token == token))
    }
    pub(in crate::process::windows::ole32) fn resume_rpc_wait(
        &mut self,
    ) -> Result<bool, HostError> {
        let Some(pending) = self.pending_call.as_ref() else {
            return Ok(false);
        };
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .rpc
                .calls
                .iter()
                .position(|call| call.owner == pending.owner && call.token == pending.token)
        }) else {
            return Ok(false);
        };
        if self.ole32.as_ref().unwrap().rpc.calls[index].frame != pending.frame {
            return Err(HostError::InvalidRequest);
        }
        self.advance_rpc_call(index)?;
        Ok(true)
    }
}
