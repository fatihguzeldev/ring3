use super::{
    ExecutionApartment32, Remote32, RemoteActivation32,
    context::{Cleanup32, ProxyPhase32},
};
use crate::{
    memory::PageRange,
    process::{
        EngineInstance, HostError,
        windows::{objects::PreparedGuestLifetime32, ole32::activation::Phase32},
    },
    windows::ole32::{ApartmentId32, Ole32Api32},
};

const DISCONNECTED: u32 = 0x8001_0108;

enum ExitAction32 {
    CancelActivation(u64),
    AbandonActivation(u64, Option<Cleanup32>),
    AbandonLocalActivation(u64, ExecutionApartment32, Cleanup32),
    DisconnectActivation(u64),
    CancelCall(u64),
    AbandonCall(u64, Option<Cleanup32>),
    DisconnectCall(u64),
}

pub(in crate::process) struct PreparedComThreadExit32 {
    thread: u32,
    ranges: Vec<PageRange>,
    actions: Vec<ExitAction32>,
    waking: Vec<u32>,
    rundown: super::rundown::PreparedComRundown32,
    transfers: super::transfer::PreparedTransferExit32,
    rpc: super::super::rpc::PreparedRpcThreadExit32,
    references: Option<PreparedGuestLifetime32>,
}
impl PreparedComThreadExit32 {
    pub(in crate::process) fn ranges(&self) -> &[PageRange] {
        &self.ranges
    }
}

impl Cleanup32 {
    pub(super) fn new(pointers: [u32; 2]) -> Self {
        let mut result = Self {
            pointers: [0; 2],
            count: 0,
            next: 0,
        };
        for pointer in pointers {
            if pointer != 0 {
                result.pointers[result.count] = pointer;
                result.count += 1;
            }
        }
        result
    }
}

impl EngineInstance {
    pub(in crate::process) fn prepare_com_thread_exit(
        &self,
        thread: u32,
    ) -> Result<PreparedComThreadExit32, HostError> {
        let mut result = PreparedComThreadExit32 {
            thread,
            transfers: self.prepare_com_transfer_exit(thread)?,
            rpc: self.prepare_rpc_thread_exit(thread)?,
            references: None,
            ranges: Vec::new(),
            actions: Vec::new(),
            waking: Vec::new(),
            rundown: self.prepare_com_rundown(
                thread,
                true,
                self.ole32
                    .as_ref()
                    .map_or(0, |runtime| runtime.activation.next),
            )?,
        };
        let Some(runtime) = self.ole32.as_ref() else {
            return Ok(result);
        };
        if !result.rpc.releases().is_empty() {
            let receipt = runtime
                .com
                .prepare_lifetime_changes(self.memory()?, &[], result.rpc.releases(), &[], &[], &[])
                .map_err(super::super::rpc::com_reference_error)?;
            result
                .ranges
                .try_reserve_exact(receipt.ranges().len())
                .map_err(|_| HostError::Infrastructure)?;
            result.ranges.extend_from_slice(receipt.ranges());
            result.references = Some(receipt);
        }
        let count = runtime
            .activation
            .pending
            .len()
            .checked_add(runtime.marshaling.calls.len())
            .and_then(|count| count.checked_add(runtime.marshaling.proxies.len()))
            .ok_or(HostError::Infrastructure)?;
        result
            .actions
            .try_reserve_exact(count)
            .map_err(|_| HostError::Infrastructure)?;
        result
            .ranges
            .try_reserve_exact(count)
            .map_err(|_| HostError::Infrastructure)?;
        result
            .waking
            .try_reserve_exact(count)
            .map_err(|_| HostError::Infrastructure)?;
        for (slot, pending) in runtime.activation.pending.iter().enumerate() {
            if self
                .com_transfer_index(super::transfer::TransferParent32::Activation(
                    pending.continuation,
                ))
                .is_some()
            {
                continue;
            }
            let remote = runtime
                .marshaling
                .activations
                .iter()
                .find(|remote| remote.continuation == pending.continuation);
            let target_dies = remote.is_some_and(|remote| remote.execution.thread == thread);
            if pending.thread == thread {
                pending
                    .scratch
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                let active = !target_dies
                    && self.has_active_com_apartment_call(thread, pending.continuation);
                let cleanup = if active {
                    None
                } else {
                    Some(self.activation_cleanup(slot, None)?)
                };
                if remote.is_none()
                    && let Some(cleanup) = cleanup.filter(|cleanup| cleanup.count != 0)
                    && let Some(execution) = self.local_activation_cleanup_apartment(slot, thread)
                {
                    result.actions.push(ExitAction32::AbandonLocalActivation(
                        pending.continuation,
                        execution,
                        cleanup,
                    ));
                } else if cleanup.is_some_and(|cleanup| cleanup.count == 0)
                    || remote.is_none()
                    || target_dies
                {
                    result.ranges.push(pending.scratch.range());
                    result
                        .actions
                        .push(ExitAction32::CancelActivation(pending.continuation));
                } else {
                    result.actions.push(ExitAction32::AbandonActivation(
                        pending.continuation,
                        cleanup,
                    ));
                }
            } else if target_dies {
                pending
                    .scratch
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                result
                    .actions
                    .push(ExitAction32::DisconnectActivation(pending.continuation));
                result.waking.push(pending.thread);
            }
        }
        for (index, call) in runtime.marshaling.calls.iter().enumerate() {
            if self
                .com_transfer_index(super::transfer::TransferParent32::Proxy(call.continuation))
                .is_some()
            {
                continue;
            }
            if call.thread == thread {
                call.scratch
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                let active = call.execution.thread != thread
                    && self.has_active_com_apartment_call(thread, call.continuation);
                let cleanup = if active {
                    None
                } else {
                    Some(self.proxy_call_cleanup(index, None)?)
                };
                if cleanup.is_some_and(|cleanup| cleanup.count == 0)
                    || call.execution.thread == thread
                {
                    result.ranges.push(call.scratch.range());
                    result
                        .actions
                        .push(ExitAction32::CancelCall(call.continuation));
                } else {
                    result
                        .actions
                        .push(ExitAction32::AbandonCall(call.continuation, cleanup));
                }
            } else if call.execution.thread == thread {
                call.scratch
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                result
                    .actions
                    .push(ExitAction32::DisconnectCall(call.continuation));
                result.waking.push(call.thread);
            }
        }
        for proxy in runtime.marshaling.proxies.iter().flatten() {
            if proxy.execution.thread == thread
                && let Some(rundown) = proxy.rundown
            {
                result.waking.push(rundown.sender);
            }
        }
        Ok(result)
    }

    pub(in crate::process) fn check_com_thread_exit(
        &self,
        prepared: &PreparedComThreadExit32,
    ) -> Result<(), HostError> {
        if let Some(receipt) = &prepared.references {
            self.ole32
                .as_ref()
                .ok_or(HostError::Infrastructure)?
                .com
                .can_commit_lifetime_changes(self.memory()?, receipt)
                .map_err(super::super::rpc::com_reference_error)?;
        }
        Ok(())
    }

    pub(in crate::process) fn commit_com_thread_exit_after_memory(
        &mut self,
        mut prepared: PreparedComThreadExit32,
        memory_changed: bool,
    ) {
        if let Some(receipt) = prepared.references.take() {
            let runtime = self.ole32.as_mut().unwrap();
            let destroyed = runtime.com.commit_lifetime_after_aggregate_unmap(
                self.memory.as_ref().unwrap(),
                receipt,
                memory_changed,
            );
            runtime.rpc.record_destroyed_identities(&destroyed);
        }
        self.commit_com_thread_exit_metadata(prepared);
    }

    #[cfg(test)]
    pub(in crate::process) fn commit_com_thread_exit(&mut self, prepared: PreparedComThreadExit32) {
        self.check_com_thread_exit(&prepared).unwrap();
        assert!(
            prepared.references.is_none(),
            "reference retirement uses the aggregate RAM path"
        );
        self.commit_com_thread_exit_metadata(prepared);
    }

    fn commit_com_thread_exit_metadata(&mut self, prepared: PreparedComThreadExit32) {
        let Some(runtime) = self.ole32.as_mut() else {
            return;
        };
        for action in prepared.actions {
            match action {
                ExitAction32::CancelActivation(id) => {
                    runtime
                        .activation
                        .pending
                        .retain(|pending| pending.continuation != id);
                    runtime
                        .marshaling
                        .activations
                        .retain(|remote| remote.continuation != id);
                }
                ExitAction32::AbandonActivation(id, cleanup) => {
                    let remote = runtime
                        .marshaling
                        .activations
                        .iter_mut()
                        .find(|remote| remote.continuation == id)
                        .unwrap();
                    remote.abandoned = true;
                    remote.cleanup = cleanup;
                    if cleanup.is_some() {
                        remote.remote = Remote32::Idle;
                        remote.next = None;
                    }
                }
                ExitAction32::AbandonLocalActivation(id, execution, cleanup) => {
                    assert!(
                        runtime.marshaling.activations.len()
                            < runtime.marshaling.activations.capacity()
                    );
                    runtime.marshaling.activations.push(RemoteActivation32 {
                        continuation: id,
                        client: execution.apartment,
                        execution,
                        remote: Remote32::Idle,
                        next: None,
                        abandoned: true,
                        cleanup: Some(cleanup),
                    });
                }
                ExitAction32::DisconnectActivation(id) => {
                    let pending = runtime
                        .activation
                        .pending
                        .iter_mut()
                        .find(|pending| pending.continuation == id)
                        .unwrap();
                    pending.phase = Phase32::Finish {
                        result: DISCONNECTED,
                    };
                    let remote = runtime
                        .marshaling
                        .activations
                        .iter_mut()
                        .find(|remote| remote.continuation == id)
                        .unwrap();
                    remote.remote = Remote32::Idle;
                    remote.next = None;
                }
                ExitAction32::CancelCall(id) => runtime
                    .marshaling
                    .calls
                    .retain(|call| call.continuation != id),
                ExitAction32::AbandonCall(id, cleanup) => {
                    let call = runtime
                        .marshaling
                        .calls
                        .iter_mut()
                        .find(|call| call.continuation == id)
                        .unwrap();
                    call.abandoned = true;
                    call.cleanup = cleanup;
                    if cleanup.is_some() {
                        call.remote = Remote32::Idle;
                    }
                }
                ExitAction32::DisconnectCall(id) => {
                    let call = runtime
                        .marshaling
                        .calls
                        .iter_mut()
                        .find(|call| call.continuation == id)
                        .unwrap();
                    call.phase = ProxyPhase32::Publish {
                        result: if call.method.0 == 2 { 0 } else { DISCONNECTED },
                    };
                    call.remote = Remote32::Idle;
                }
            }
        }
        for proxy in runtime.marshaling.proxies.iter_mut().flatten() {
            if proxy.client == ApartmentId32::Sta(prepared.thread)
                || proxy.execution.thread == prepared.thread
            {
                proxy.disconnected = true;
                if proxy.execution.thread == prepared.thread {
                    proxy.rundown = None;
                }
            }
        }
        self.commit_rpc_thread_exit(prepared.rpc);
        self.commit_com_transfer_exit(prepared.transfers);
        self.commit_com_rundown(prepared.rundown);
        self.retire_com_sender_invocations(prepared.thread);
        for thread in prepared.waking {
            self.wake_com_apartment_sender(thread);
        }
    }

    fn local_com_activation_result(
        &self,
        pending: &crate::process::windows::ole32::activation::PendingActivation32,
    ) -> Option<u32> {
        use crate::process::windows::callbacks::WindowsCallbackProvider32;
        if self.threads.current_id() == pending.thread
            && let Some(result) = self.windows_replies.iter().find_map(|reply| {
                reply.received_result(
                    WindowsCallbackProvider32::Ole32,
                    pending.owner,
                    pending.token,
                    pending.continuation,
                    pending.frame,
                )
            })
        {
            return Some(result);
        }
        self.threads.windows_callback_result(
            pending.thread,
            WindowsCallbackProvider32::Ole32,
            pending.owner,
            pending.token,
            pending.continuation,
            pending.frame,
        )
    }

    fn local_activation_cleanup_apartment(
        &self,
        slot: usize,
        dying: u32,
    ) -> Option<ExecutionApartment32> {
        let runtime = self.ole32.as_ref()?;
        let pending = &runtime.activation.pending[slot];
        let crate::windows::ole32::ApartmentTarget32::Current(apartment) = runtime
            .apartments
            .server_target(pending.thread, pending.threading)
            .ok()?
        else {
            return None;
        };
        if matches!(apartment, ApartmentId32::Sta(_)) {
            return None;
        }
        let thread = runtime.apartments.mta_threads().find(|thread| {
            *thread != dying
                && self.windows_thread_info(*thread).is_ok_and(|info| {
                    info.state != crate::windows::thread::ThreadState32::Terminated
                })
        })?;
        Some(ExecutionApartment32 { apartment, thread })
    }

    pub(super) fn activation_cleanup(
        &self,
        slot: usize,
        completed: Option<u32>,
    ) -> Result<Cleanup32, HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let pending = &runtime.activation.pending[slot];
        let remote = runtime
            .marshaling
            .activations
            .iter()
            .find(|remote| remote.continuation == pending.continuation);
        let returned = completed.or_else(|| {
            if let Some(remote) = remote {
                if remote.next.is_none()
                    && let Remote32::Ready(result) = remote.remote
                {
                    return Some(result);
                }
                None
            } else {
                self.local_com_activation_result(pending)
            }
        });
        let output = || {
            self.ole32_read_word(if remote.is_some() {
                pending.scratch.address.0 + 52
            } else {
                pending.out
            })
        };
        let word = |at: u32| self.ole32_read_word(pending.scratch.address.0 + at);
        Ok(Cleanup32::new(match pending.phase {
            Phase32::Factory if returned.is_some_and(|value| value & 0x8000_0000 == 0) => [
                if pending.api == Ole32Api32::CoGetClassObject {
                    output()?
                } else {
                    word(48)?
                },
                0,
            ],
            Phase32::Create { factory, .. } => [
                factory,
                if returned.is_some_and(|value| value & 0x8000_0000 == 0) {
                    output()?
                } else {
                    0
                },
            ],
            Phase32::Release { result } => [
                if returned.is_none() { word(48)? } else { 0 },
                if result & 0x8000_0000 == 0 {
                    output()?
                } else {
                    0
                },
            ],
            _ => [0; 2],
        }))
    }

    pub(super) fn proxy_call_cleanup(
        &self,
        index: usize,
        completed: Option<u32>,
    ) -> Result<Cleanup32, HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.marshaling.calls[index];
        let proxy = runtime.marshaling.proxies[call.proxy]
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        let returned = completed.or({
            if let Remote32::Ready(result) = call.remote {
                Some(result)
            } else {
                None
            }
        });
        Ok(Cleanup32::new(match call.phase {
            ProxyPhase32::Query { .. } | ProxyPhase32::Create { .. }
                if returned.is_some_and(|value| value & 0x8000_0000 == 0) =>
            {
                [self.ole32_read_word(call.scratch.address.0 + 16)?, 0]
            }
            ProxyPhase32::Release { next } => [
                if next == 0 && returned.is_none() {
                    proxy.object
                } else {
                    0
                },
                if proxy.factory_extra && (next == 0 || returned.is_none()) {
                    proxy.factory.unwrap()
                } else {
                    0
                },
            ],
            _ => [0; 2],
        }))
    }
}
