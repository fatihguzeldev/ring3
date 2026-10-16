use super::{ExecutionApartment32, Remote32};
use crate::process::{EngineInstance, HostError};

impl EngineInstance {
    pub(in crate::process) fn com_apartment_invocation_eligible(&self, thread: u32) -> bool {
        if self.stream_copy_queued(thread)
            || self.com_apartment_host_idle(thread)
            || self.rpc_same_thread_invocation(thread)
            || self.com_same_thread_context_invocation(thread)
        {
            return true;
        }
        if self
            .ole32
            .as_ref()
            .is_none_or(|runtime| runtime.apartments.is_single_threaded(thread) != Ok(true))
        {
            return false;
        }
        if self.threads.current_id() == thread && self.waiting_user32_message().is_some() {
            return true;
        }
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.marshaling.activations.iter().any(|remote| {
                remote.remote == Remote32::Queued
                    && runtime.activation.pending.iter().any(|pending| {
                        pending.continuation == remote.continuation && pending.thread == thread
                    })
            }) || runtime
                .marshaling
                .calls
                .iter()
                .any(|call| call.thread == thread && call.remote == Remote32::Queued)
                || runtime
                    .marshaling
                    .transfers
                    .iter()
                    .any(|call| call.sender == thread && call.remote == Remote32::Queued)
                || runtime.marshaling.proxies.iter().flatten().any(|proxy| {
                    proxy.rundown.is_some_and(|rundown| {
                        rundown.sender == thread && rundown.remote == Remote32::Queued
                    })
                })
        })
    }

    pub(in crate::process) fn has_com_apartment_invocation(&self, thread: u32) -> bool {
        if self.stream_copy_queued(thread) || self.has_rpc_invocation(thread) {
            return true;
        }
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.marshaling.proxies.iter().flatten().any(|proxy| {
                proxy.execution.thread == thread
                    && proxy
                        .rundown
                        .is_some_and(|rundown| rundown.remote == Remote32::Queued)
            }) || runtime.marshaling.activations.iter().any(|remote| {
                remote.execution.thread == thread && remote.remote == Remote32::Queued
            }) || runtime
                .marshaling
                .calls
                .iter()
                .any(|call| call.execution.thread == thread && call.remote == Remote32::Queued)
                || runtime
                    .marshaling
                    .transfers
                    .iter()
                    .any(|call| call.execution.thread == thread && call.remote == Remote32::Queued)
        })
    }

    fn com_same_thread_context_invocation(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.marshaling.transfers.iter().any(|call| {
                call.sender == thread
                    && call.execution.thread == thread
                    && call.remote == Remote32::Queued
                    && (call.execution.apartment == crate::windows::ole32::ApartmentId32::Neutral
                        || runtime.apartments.identity(thread) == Ok(call.execution.apartment))
            }) || runtime.marshaling.activations.iter().any(|remote| {
                remote.execution.thread == thread
                    && remote.execution.apartment == crate::windows::ole32::ApartmentId32::Neutral
                    && remote.remote == Remote32::Queued
                    && runtime.activation.pending.iter().any(|pending| {
                        pending.continuation == remote.continuation && pending.thread == thread
                    })
            }) || runtime.marshaling.calls.iter().any(|call| {
                call.thread == thread
                    && call.execution.thread == thread
                    && call.execution.apartment == crate::windows::ole32::ApartmentId32::Neutral
                    && call.remote == Remote32::Queued
            }) || runtime.marshaling.proxies.iter().flatten().any(|proxy| {
                proxy.execution.thread == thread
                    && proxy.rundown.is_some_and(|rundown| {
                        rundown.sender == thread && rundown.remote == Remote32::Queued
                    })
            })
        })
    }

    fn com_invocation_execution(
        &self,
        sender: u32,
        continuation: u64,
    ) -> Result<ExecutionApartment32, HostError> {
        if let Some(execution) = self.stream_copy_invocation(sender, continuation) {
            return execution;
        }
        if let Some(execution) = self.rpc_invocation_execution(sender, continuation) {
            return execution;
        }
        let runtime = self.ole32.as_ref().ok_or(HostError::InvalidRequest)?;
        if let Some(call) = runtime.marshaling.transfers.iter().find(|call| {
            call.sender == sender
                && call.continuation == continuation
                && call.remote == Remote32::Queued
        }) {
            self.validate_com_transfer_identity(call)?;
            if let Some(scratch) = call.scratch {
                scratch.check(self.memory()?).map_err(HostError::Memory)?;
            }
            return Ok(call.execution);
        }
        if let Some(remote) =
            runtime.marshaling.activations.iter().find(|remote| {
                remote.continuation == continuation && remote.remote == Remote32::Queued
            })
        {
            let pending = runtime
                .activation
                .pending
                .iter()
                .find(|pending| pending.continuation == continuation && pending.thread == sender)
                .ok_or(HostError::InvalidRequest)?;
            pending
                .scratch
                .check(self.memory()?)
                .map_err(HostError::Memory)?;
            return Ok(remote.execution);
        }
        if let Some(proxy) = runtime.marshaling.proxies.iter().flatten().find(|proxy| {
            proxy.rundown.is_some_and(|rundown| {
                rundown.sender == sender
                    && rundown.continuation == continuation
                    && rundown.remote == Remote32::Queued
            })
        }) {
            return Ok(proxy.execution);
        }
        let call = runtime
            .marshaling
            .calls
            .iter()
            .find(|call| {
                call.continuation == continuation
                    && call.thread == sender
                    && call.remote == Remote32::Queued
            })
            .ok_or(HostError::InvalidRequest)?;
        call.scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        Ok(call.execution)
    }

    pub(in crate::process) fn check_com_apartment_invocation(
        &self,
        sender: u32,
        continuation: u64,
    ) -> Result<(), HostError> {
        let execution = self.com_invocation_execution(sender, continuation)?;
        if execution.thread != self.threads.current_id() {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    pub(in crate::process) fn prepare_com_apartment_entry(
        &self,
        sender: u32,
        continuation: u64,
    ) -> Result<(), HostError> {
        self.check_com_apartment_invocation(sender, continuation)?;
        let execution = self.com_invocation_execution(sender, continuation)?;
        self.ole32
            .as_ref()
            .unwrap()
            .apartments
            .check_context_entry(execution.thread, execution.apartment)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(())
    }

    pub(in crate::process) fn enter_com_apartment_invocation(
        &mut self,
        sender: u32,
        continuation: u64,
    ) {
        let execution = self
            .com_invocation_execution(sender, continuation)
            .expect("COM invocation identity and scratch were preflighted");
        self.ole32
            .as_mut()
            .unwrap()
            .apartments
            .enter_context(execution.thread, execution.apartment);
    }

    pub(in crate::process) fn leave_com_apartment_invocation(
        &mut self,
        sender: u32,
        continuation: u64,
    ) {
        let execution = self
            .com_invocation_execution(sender, continuation)
            .expect("COM invocation owns its apartment until completion");
        self.ole32
            .as_mut()
            .unwrap()
            .apartments
            .leave_context(execution.thread, execution.apartment);
    }

    pub(in crate::process) fn complete_com_apartment_invocation(
        &mut self,
        sender: u32,
        continuation: u64,
        result: u32,
    ) -> Result<(), HostError> {
        self.check_com_apartment_invocation(sender, continuation)?;
        if self.complete_stream_copy_invocation(sender, continuation, result)
            || self.complete_rpc_invocation(sender, continuation, result)
        {
            return Ok(());
        }
        if let Some(index) = self
            .ole32
            .as_ref()
            .unwrap()
            .marshaling
            .transfers
            .iter()
            .position(|call| {
                call.sender == sender
                    && call.continuation == continuation
                    && call.remote == Remote32::Queued
            })
        {
            let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
            if let super::transfer::TransferPhase32::Release(next) = call.phase {
                call.references[next].pointer = 0;
            }
            call.remote = Remote32::Ready(result);
            self.wake_com_apartment_sender(sender);
            return Ok(());
        }
        if let Some(proxy) = self
            .ole32
            .as_mut()
            .unwrap()
            .marshaling
            .proxies
            .iter_mut()
            .flatten()
            .find(|proxy| {
                proxy.rundown.is_some_and(|rundown| {
                    rundown.sender == sender
                        && rundown.continuation == continuation
                        && rundown.remote == Remote32::Queued
                })
            })
        {
            let rundown = proxy.rundown.as_mut().unwrap();
            proxy.owned[rundown.next] = 0;
            rundown.next += 1;
            rundown.remote = Remote32::Ready(result);
            self.wake_com_apartment_sender(sender);
            return Ok(());
        }
        let activation = self
            .ole32
            .as_ref()
            .unwrap()
            .activation
            .pending
            .iter()
            .position(|pending| pending.continuation == continuation && pending.thread == sender);
        let abandoned_cleanup = if let Some(slot) = activation {
            let remote = self
                .ole32
                .as_ref()
                .unwrap()
                .marshaling
                .activations
                .iter()
                .find(|remote| remote.continuation == continuation)
                .unwrap();
            if remote.abandoned && remote.cleanup.is_none() {
                Some(self.activation_cleanup(slot, Some(result))?)
            } else {
                None
            }
        } else {
            let index = self
                .ole32
                .as_ref()
                .unwrap()
                .marshaling
                .calls
                .iter()
                .position(|call| call.continuation == continuation && call.thread == sender)
                .unwrap();
            let call = &self.ole32.as_ref().unwrap().marshaling.calls[index];
            if call.abandoned && call.cleanup.is_none() {
                Some(self.proxy_call_cleanup(index, Some(result))?)
            } else {
                None
            }
        };
        let runtime = self.ole32.as_mut().unwrap();
        if let Some(remote) = runtime
            .marshaling
            .activations
            .iter_mut()
            .find(|remote| remote.continuation == continuation)
        {
            if remote.abandoned {
                if let Some(cleanup) = abandoned_cleanup {
                    remote.cleanup = Some(cleanup);
                } else {
                    remote.cleanup.as_mut().unwrap().next += 1;
                }
            }
            remote.remote = Remote32::Ready(result);
        } else {
            let call = runtime
                .marshaling
                .calls
                .iter_mut()
                .find(|call| call.continuation == continuation && call.thread == sender)
                .unwrap();
            if let super::context::ProxyPhase32::Release { next } = call.phase
                && call.cleanup.is_none()
            {
                runtime.marshaling.proxies[call.proxy]
                    .as_mut()
                    .unwrap()
                    .owned[next] = 0;
            }
            if call.abandoned {
                if let Some(cleanup) = abandoned_cleanup {
                    call.cleanup = Some(cleanup);
                } else {
                    call.cleanup.as_mut().unwrap().next += 1;
                }
            }
            call.remote = Remote32::Ready(result);
        }
        self.wake_com_apartment_sender(sender);
        Ok(())
    }
}
