use super::{
    Remote32,
    context::{ProxyCall32, ProxyPhase32},
    proxy::proxy_memory_error,
};
use crate::{
    memory::{Access, GuestAddress, WordWrite32},
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{
                activation::ActivationRuntime32,
                memory::{iid, output},
            },
        },
    },
    windows::{
        CallFrame32,
        com::{IID_IUNKNOWN, InterfaceId},
        ole32::{ApartmentId32, FactoryMethod32, HResult32, IID_ICLASSFACTORY},
    },
};

impl EngineInstance {
    pub(in crate::process) fn com_proxy_wait_ready(&self, thread: u32) -> bool {
        self.stream_copy_ready(thread)
            || self.rpc_work_ready(thread)
            || self.com_transfer_work_ready(thread)
            || self.ole32.as_ref().is_some_and(|runtime| {
                runtime
                    .marshaling
                    .calls
                    .iter()
                    .enumerate()
                    .any(|(index, call)| {
                        call.thread == thread
                            && self.proxy_call_first(index)
                            && call.remote != Remote32::Queued
                            && self
                                .com_transfer_parent_ready(
                                    super::transfer::TransferParent32::Proxy(call.continuation),
                                )
                                .unwrap_or(true)
                    })
            })
    }
    pub(in crate::process) fn resume_com_proxy_wait(&mut self) -> Result<(), HostError> {
        if self.resume_stream_copy()?
            || self.resume_com_transfer_work()?
            || self.resume_rpc_wait()?
        {
            return Ok(());
        }
        let Some(pending) = self.pending_call.as_ref() else {
            return Ok(());
        };
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime.marshaling.calls.iter().position(|call| {
                call.thread == self.threads.current_id()
                    && call.owner == pending.owner
                    && call.token == pending.token
            })
        }) else {
            return Ok(());
        };
        self.advance_com_proxy_call(index)
    }
    pub(in crate::process::windows::ole32) fn com_proxy_owns_token(&self, token: u32) -> bool {
        self.stream_copy_owns_token(token)
            || self.rpc_owns_token(token)
            || self.ole32.as_ref().is_some_and(|runtime| {
                runtime
                    .marshaling
                    .calls
                    .iter()
                    .any(|call| call.token == token)
            })
    }
    pub(in crate::process::windows::ole32) fn com_proxy_pending_owners(
        &self,
    ) -> impl Iterator<Item = PendingOwner> + '_ {
        self.ole32
            .iter()
            .flat_map(|runtime| runtime.marshaling.calls.iter().map(|call| call.owner))
            .chain(self.rpc_pending_owners())
            .chain(self.stream_copy_pending_owners())
    }
    pub(in crate::process::windows::ole32) fn try_complete_com_proxy_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        method: FactoryMethod32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        if let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .marshaling
                .calls
                .iter()
                .position(|call| call.owner == owner && call.token == token)
        }) {
            if self.ole32.as_ref().unwrap().marshaling.calls[index].frame != frame {
                return Err(HostError::InvalidRequest);
            }
            self.advance_com_proxy_call(index)?;
            return Ok(true);
        }
        let Some(index) = self
            .ole32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .proxy_index(self.memory()?, frame.arguments()[0])?
        else {
            return Ok(false);
        };
        let thread = self.current_windows_thread_id()?;
        let args = frame.arguments();
        let runtime = self.ole32.as_ref().unwrap();
        let proxy = *runtime.marshaling.proxies[index].as_ref().unwrap();
        if let Err(error) = runtime.check_proxy_client(index, thread) {
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(error.0))?);
            return Ok(true);
        }
        if method.0 >= 3 && (args[0] != proxy.factory_view.0 || proxy.factory.is_none()) {
            return Err(HostError::InvalidRequest);
        }
        if method.0 == 1
            || method.0 == 2
                && runtime
                    .com
                    .references(proxy.pointer)
                    .map_err(proxy_memory_error)?
                    > 1
            || method.0 == 2 && proxy.disconnected
        {
            let runtime = self.ole32.as_mut().unwrap();
            let result = if method.0 == 1 {
                runtime
                    .com
                    .add_ref(self.memory.as_ref().unwrap(), proxy.pointer)
                    .map_err(proxy_memory_error)?
            } else {
                let outcome = runtime
                    .com
                    .release(self.memory.as_mut().unwrap(), proxy.pointer)
                    .map_err(proxy_memory_error)?;
                if outcome.destroyed.is_some() {
                    let proxy = runtime.marshaling.proxies[index].as_mut().unwrap();
                    proxy.local_alive = false;
                    if proxy.rundown.is_none() && proxy.owned == [0; 2] {
                        runtime.marshaling.proxies[index] = None;
                    }
                }
                outcome.remaining
            };
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
            return Ok(true);
        }
        if proxy.disconnected && method.0 >= 3 {
            let out = if method.0 == 3 { Some(args[3]) } else { None };
            if let Some(out) = out {
                self.memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(out), &0u32.to_le_bytes())
                    .map_err(HostError::Memory)?;
            }
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0x8001_0108))?);
            return Ok(true);
        }
        let (phase, out, requested) = match method.0 {
            0 => {
                output(self.memory()?, args[2])
                    .map_err(|_| HostError::Memory(crate::memory::MemoryError::InvalidRange))?;
                let requested =
                    iid(self.memory()?, args[1]).map_err(|_| HostError::InvalidRequest)?;
                if requested == IID_IUNKNOWN
                    || requested == IID_ICLASSFACTORY && proxy.factory.is_some()
                {
                    self.publish_proxy_query(index, requested, args[2])?;
                    self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0))?);
                    return Ok(true);
                }
                if proxy.disconnected {
                    self.memory
                        .as_mut()
                        .unwrap()
                        .write(GuestAddress(args[2]), &0u32.to_le_bytes())
                        .map_err(HostError::Memory)?;
                    self.publish_prepared_call(Self::prepare_completed_call(
                        frame.complete(0x8001_0108),
                    )?);
                    return Ok(true);
                }
                (ProxyPhase32::Query { requested }, args[2], requested)
            }
            2 => (ProxyPhase32::Release { next: 0 }, 0, IID_IUNKNOWN),
            3 => {
                output(self.memory()?, args[3]).map_err(|_| HostError::InvalidRequest)?;
                if args[1] != 0 {
                    self.memory
                        .as_mut()
                        .unwrap()
                        .write(GuestAddress(args[3]), &0u32.to_le_bytes())
                        .map_err(HostError::Memory)?;
                    self.publish_prepared_call(Self::prepare_completed_call(
                        frame.complete(HResult32::NO_AGGREGATION.0),
                    )?);
                    return Ok(true);
                }
                let requested =
                    iid(self.memory()?, args[2]).map_err(|_| HostError::InvalidRequest)?;
                (ProxyPhase32::Create { requested }, args[3], requested)
            }
            4 => (ProxyPhase32::Lock, 0, IID_IUNKNOWN),
            _ => return Err(HostError::InvalidRequest),
        };
        let mut execution = proxy.execution;
        if execution.apartment == ApartmentId32::Neutral {
            execution.thread = thread;
        } else if execution.apartment == ApartmentId32::Mta {
            execution.thread = self.ensure_com_apartment_host(false)?;
        }
        if self.ole32.as_ref().unwrap().marshaling.calls.len() >= 64 {
            return Err(HostError::Infrastructure);
        }
        self.ole32
            .as_mut()
            .unwrap()
            .marshaling
            .calls
            .try_reserve(1)
            .map_err(|_| HostError::Infrastructure)?;
        let continuation = self
            .ole32
            .as_mut()
            .unwrap()
            .activation
            .prepare()
            .map_err(|_| HostError::Infrastructure)?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let mut bytes = [0u8; 64];
        bytes[..16].copy_from_slice(&requested.0);
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            ActivationRuntime32::scratch_reservation(),
            &bytes,
        )
        .map_err(HostError::Memory)?;
        if out != 0 {
            self.memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(out), &0u32.to_le_bytes())
                .map_err(HostError::Memory)?;
        }
        let runtime = self.ole32.as_mut().unwrap();
        runtime.activation.next = continuation;
        runtime.marshaling.calls.push(ProxyCall32 {
            continuation,
            thread,
            owner,
            token,
            frame,
            method,
            proxy: index,
            execution,
            out,
            scratch,
            phase,
            remote: Remote32::Idle,
            abandoned: false,
            cleanup: None,
        });
        self.advance_com_proxy_call(self.ole32.as_ref().unwrap().marshaling.calls.len() - 1)?;
        Ok(true)
    }

    fn publish_proxy_query(
        &mut self,
        index: usize,
        requested: InterfaceId,
        out: u32,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_mut().unwrap();
        let proxy = runtime.marshaling.proxies[index].as_ref().unwrap();
        let pointer = if requested == IID_IUNKNOWN {
            proxy.unknown
        } else {
            proxy.factory_view
        };
        let memory = self.memory.as_mut().unwrap();
        let prepared = runtime
            .com
            .prepare_lifetime_changes(
                memory,
                &[proxy.pointer],
                &[],
                &[],
                &[],
                &[WordWrite32 {
                    address: GuestAddress(out),
                    value: pointer.0,
                }],
            )
            .map_err(proxy_memory_error)?;
        runtime
            .com
            .commit_lifetime_changes(memory, prepared)
            .map_err(proxy_memory_error)?;
        Ok(())
    }

    pub(in crate::process::windows::ole32) fn com_guest_method(
        &self,
        object: u32,
        slot: u32,
    ) -> Result<GuestAddress, HostError> {
        let table = self.ole32_read_word(object)?;
        let offset = slot
            .checked_mul(4)
            .and_then(|offset| table.checked_add(offset))
            .ok_or(HostError::InvalidRequest)?;
        let procedure = GuestAddress(self.ole32_read_word(offset)?);
        self.memory()?
            .check_span(procedure, 1, Access::Execute)
            .map_err(HostError::Memory)?;
        Ok(procedure)
    }

    fn proxy_call_first(&self, index: usize) -> bool {
        let calls = &self.ole32.as_ref().unwrap().marshaling.calls;
        !calls.iter().any(|other| {
            other.proxy == calls[index].proxy && other.continuation < calls[index].continuation
        })
    }

    pub(in crate::process::windows::ole32) fn advance_com_proxy_call(
        &mut self,
        index: usize,
    ) -> Result<(), HostError> {
        if matches!(
            self.ole32.as_ref().unwrap().marshaling.calls[index].phase,
            ProxyPhase32::Transfer { .. }
        ) {
            return self.finish_com_proxy_transfer(index);
        }
        if !self.proxy_call_first(index) {
            return Ok(());
        }
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.marshaling.calls[index];
        self.checked_pending_call(call.owner, call.token)?;
        call.scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        if let Remote32::Ready(result) = call.remote {
            return self.accept_com_proxy_result(index, result);
        }
        if matches!(call.phase, ProxyPhase32::Publish { .. }) {
            return self.finish_com_proxy_call(index);
        }
        if call.remote == Remote32::Queued {
            return Ok(());
        }
        let proxy = runtime.marshaling.proxies[call.proxy]
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        let scratch = call.scratch.address.0;
        let (object, slot, arguments, count) = match call.phase {
            ProxyPhase32::Query { .. } => {
                (proxy.object, 0, [proxy.object, scratch, scratch + 16, 0], 3)
            }
            ProxyPhase32::Create { .. } => (
                proxy.factory.unwrap(),
                3,
                [proxy.factory.unwrap(), 0, scratch, scratch + 16],
                4,
            ),
            ProxyPhase32::Lock => (
                proxy.factory.unwrap(),
                4,
                [proxy.factory.unwrap(), call.frame.arguments()[1], 0, 0],
                2,
            ),
            ProxyPhase32::Release { next } => {
                let object = if next == 0 {
                    proxy.object
                } else {
                    proxy
                        .factory
                        .filter(|_| proxy.factory_extra)
                        .ok_or(HostError::InvalidRequest)?
                };
                (object, 2, [object, 0, 0, 0], 1)
            }
            ProxyPhase32::Publish { .. } | ProxyPhase32::Transfer { .. } => unreachable!(),
        };
        let (target, sender, continuation) =
            (call.execution.thread, call.thread, call.continuation);
        let procedure = self.com_guest_method(object, slot)?;
        self.queue_com_apartment_invocation(
            target,
            procedure,
            &arguments[..count],
            sender,
            continuation,
        )?;
        self.ole32.as_mut().unwrap().marshaling.calls[index].remote = Remote32::Queued;
        Ok(())
    }

    fn accept_com_proxy_result(&mut self, index: usize, result: u32) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.marshaling.calls[index];
        let proxy = *runtime.marshaling.proxies[call.proxy]
            .as_ref()
            .ok_or(HostError::InvalidRequest)?;
        let phase = call.phase;
        let proxy_index = call.proxy;
        let out = call.out;
        let next = match phase {
            ProxyPhase32::Query { requested } | ProxyPhase32::Create { requested }
                if result & 0x8000_0000 == 0 =>
            {
                let pointer = self.ole32_read_word(call.scratch.address.0 + 16)?;
                if pointer == 0 {
                    ProxyPhase32::Publish {
                        result: HResult32::UNEXPECTED.0,
                    }
                } else if requested != IID_IUNKNOWN && requested != IID_ICLASSFACTORY {
                    return self.begin_com_proxy_transfer(index, result, pointer, requested);
                } else if matches!(phase, ProxyPhase32::Query { .. }) {
                    self.memory()?
                        .ensure_transaction_versions(2)
                        .map_err(HostError::Memory)?;
                    self.publish_proxy_query(proxy_index, requested, out)?;
                    let entry = self.ole32.as_mut().unwrap().marshaling.proxies[proxy_index]
                        .as_mut()
                        .unwrap();
                    entry.factory = Some(pointer);
                    entry.factory_extra = true;
                    entry.owned[1] = pointer;
                    ProxyPhase32::Publish { result }
                } else {
                    let (execution, out) = (call.execution, call.out);
                    self.memory()?
                        .ensure_transaction_versions(2)
                        .map_err(HostError::Memory)?;
                    self.publish_standard_com_proxy(
                        execution,
                        proxy.client,
                        proxy.server,
                        pointer,
                        requested,
                        out,
                    )?;
                    ProxyPhase32::Publish { result }
                }
            }
            ProxyPhase32::Release { next: 0 } if proxy.factory_extra => {
                ProxyPhase32::Release { next: 1 }
            }
            ProxyPhase32::Release { .. } => ProxyPhase32::Publish { result: 0 },
            ProxyPhase32::Publish { .. } => return self.finish_com_proxy_call(index),
            ProxyPhase32::Transfer { .. } => return self.finish_com_proxy_transfer(index),
            _ => ProxyPhase32::Publish { result },
        };
        let call = &mut self.ole32.as_mut().unwrap().marshaling.calls[index];
        call.phase = next;
        call.remote = Remote32::Idle;
        self.advance_com_proxy_call(index)
    }

    fn finish_com_proxy_call(&mut self, index: usize) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.calls[index];
        let ProxyPhase32::Publish { result } = call.phase else {
            return Err(HostError::InvalidRequest);
        };
        let frame = call.frame;
        let proxy_index = call.proxy;
        call.scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        if call.method.0 == 2 {
            let runtime = self.ole32.as_mut().unwrap();
            let proxy = runtime.marshaling.proxies[proxy_index].as_ref().unwrap();
            runtime
                .com
                .release(self.memory.as_mut().unwrap(), proxy.pointer)
                .map_err(proxy_memory_error)?;
            runtime.marshaling.proxies[proxy_index] = None;
        }
        self.ole32.as_ref().unwrap().marshaling.calls[index]
            .scratch
            .release(self.memory.as_mut().unwrap())
            .map_err(HostError::Memory)?;
        self.ole32.as_mut().unwrap().marshaling.calls.remove(index);
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
        Ok(())
    }
}
