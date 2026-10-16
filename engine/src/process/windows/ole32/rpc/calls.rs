use super::{
    buffers::read_message,
    context::{CHANNEL_PAYLOAD, CONTROLLER_PAYLOAD, RpcCall32, RpcPhase32},
};
use crate::{
    memory::{Access, GuestAddress, WordWrite32},
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            objects::GuestComError32,
            ole32::{marshaling::Remote32, memory::iid},
        },
    },
    windows::{
        CallFrame32, CallingConvention32,
        com::IID_IUNKNOWN,
        ole32::{FactoryMethod32, HResult32, RpcChannelMethod32, rpc::IID_IRPCCHANNELBUFFER},
    },
};
pub(in crate::process::windows::ole32) fn com_error(error: GuestComError32) -> HostError {
    match error {
        GuestComError32::Memory(error) => HostError::Memory(error),
        _ => HostError::Infrastructure,
    }
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn try_complete_com_rpc_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let channel_method = RpcChannelMethod32::from_id(id);
        let controller_method = FactoryMethod32::from_id(id).filter(|method| method.0 < 3);
        if channel_method.is_none() && controller_method.is_none() {
            return Ok(false);
        }
        if let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .rpc
                .calls
                .iter()
                .position(|call| call.owner == owner && call.token == token)
        }) {
            let call = &self.ole32.as_ref().unwrap().rpc.calls[index];
            self.checked_pending_call(owner, token)?;
            if call.frame != frame {
                return Err(HostError::InvalidRequest);
            }
            self.advance_rpc_call(index)?;
            return Ok(true);
        }
        let Some(runtime) = self.ole32.as_ref() else {
            return Ok(false);
        };
        let pointer = frame
            .arguments()
            .first()
            .copied()
            .ok_or(HostError::Call(CallError::InvalidRequest))?;
        let Ok(identity) = runtime.com.lookup(GuestAddress(pointer)) else {
            return Ok(false);
        };
        let tag = identity.payload & !(CONTROLLER_PAYLOAD - 1);
        if ![CONTROLLER_PAYLOAD, CHANNEL_PAYLOAD].contains(&tag) {
            return Ok(false);
        }
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        let method = if tag == CHANNEL_PAYLOAD {
            channel_method.map(|method| (method.0, method.stack_words()))
        } else {
            controller_method.map(|method| (method.0, method.stack_words()))
        }
        .ok_or(HostError::InvalidRequest)?;
        if frame.convention() != CallingConvention32::Stdcall || frame.stack_words() != method.1 {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        runtime
            .com
            .preflight(self.memory()?, GuestAddress(pointer))
            .map_err(com_error)?;
        let index = (identity.payload & (CONTROLLER_PAYLOAD - 1)) as usize;
        let broker = runtime
            .rpc
            .brokers
            .get(index)
            .and_then(Option::as_ref)
            .copied()
            .ok_or(HostError::InvalidRequest)?;
        let thread = self.current_windows_thread_id()?;
        let permitted = runtime.rpc.apartment_matches(
            index,
            runtime
                .apartments
                .identity(thread)
                .map_err(|_| HostError::InvalidRequest)?,
        ) || tag == CHANNEL_PAYLOAD
            && self.active_rpc_invoke(index, thread).is_some();
        if !permitted {
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0x8001010e))?);
            return Ok(true);
        }
        let channel_lease = tag == CHANNEL_PAYLOAD && self.rpc_channel_lease_scope(index, thread);
        let args = frame.arguments();
        let result = match method.0 {
            0 => {
                let requested =
                    iid(self.memory()?, args[1]).map_err(|_| HostError::InvalidRequest)?;
                let target = if requested == IID_IUNKNOWN {
                    Some(
                        runtime
                            .com
                            .query_target(self.memory()?, GuestAddress(pointer), IID_IUNKNOWN)
                            .map_err(com_error)?,
                    )
                } else if tag == CHANNEL_PAYLOAD && requested == IID_IRPCCHANNELBUFFER {
                    broker.channel
                } else if tag == CONTROLLER_PAYLOAD
                    && requested == broker.requested
                    && broker.interface != 0
                {
                    Some(GuestAddress(broker.interface))
                } else {
                    None
                };
                if tag == CONTROLLER_PAYLOAD && target.is_none() {
                    self.begin_rpc_query(owner, token, frame, index, requested)?;
                    return Ok(true);
                }
                let retained = [GuestAddress(pointer)];
                let lease_references = if channel_lease && target.is_some() {
                    Some(
                        broker
                            .proxy_channel_references
                            .checked_add(1)
                            .ok_or(HostError::Infrastructure)?,
                    )
                } else {
                    None
                };
                let runtime = self.ole32.as_mut().unwrap();
                let receipt = runtime
                    .com
                    .prepare_lifetime_changes(
                        self.memory.as_ref().unwrap(),
                        if target.is_some() { &retained } else { &[] },
                        &[],
                        &[],
                        &[],
                        &[WordWrite32 {
                            address: GuestAddress(args[2]),
                            value: target.map_or(0, |pointer| pointer.0),
                        }],
                    )
                    .map_err(com_error)?;
                runtime
                    .com
                    .commit_lifetime_changes(self.memory.as_mut().unwrap(), receipt)
                    .map_err(com_error)?;
                if let Some(references) = lease_references {
                    runtime.rpc.brokers[index]
                        .as_mut()
                        .unwrap()
                        .proxy_channel_references = references;
                }
                if target.is_some() {
                    0
                } else {
                    HResult32::NO_INTERFACE.0
                }
            }
            1 => {
                let lease_references = if channel_lease {
                    Some(
                        broker
                            .proxy_channel_references
                            .checked_add(1)
                            .ok_or(HostError::Infrastructure)?,
                    )
                } else {
                    None
                };
                let runtime = self.ole32.as_mut().unwrap();
                let remaining = runtime
                    .com
                    .add_ref(self.memory.as_ref().unwrap(), GuestAddress(pointer))
                    .map_err(com_error)?;
                if let Some(references) = lease_references {
                    runtime.rpc.brokers[index]
                        .as_mut()
                        .unwrap()
                        .proxy_channel_references = references;
                }
                remaining
            }
            2 => {
                let final_controller = tag == CONTROLLER_PAYLOAD
                    && runtime
                        .com
                        .references(GuestAddress(pointer))
                        .map_err(com_error)?
                        == 1;
                if final_controller
                    && broker.build == super::context::BuildPhase32::Publish
                    && broker.rundown.is_none()
                {
                    self.begin_rpc_cleanup(owner, token, frame, index)?;
                    return Ok(true);
                }
                let runtime = self.ole32.as_mut().unwrap();
                let outcome = runtime
                    .com
                    .release(self.memory.as_mut().unwrap(), GuestAddress(pointer))
                    .map_err(com_error)?;
                if channel_lease {
                    runtime.rpc.brokers[index]
                        .as_mut()
                        .unwrap()
                        .proxy_channel_references =
                        broker.proxy_channel_references.saturating_sub(1);
                }
                if outcome.destroyed.is_some() {
                    let broker = runtime.rpc.brokers[index].as_mut().unwrap();
                    if tag == CONTROLLER_PAYLOAD {
                        broker.controller_live = false
                    } else {
                        broker.channel_live = false
                    }
                }
                runtime.rpc.retire_disconnected_broker(index);
                outcome.remaining
            }
            3 => {
                let requested =
                    iid(self.memory()?, args[2]).map_err(|_| HostError::InvalidRequest)?;
                if requested != broker.requested {
                    HResult32::NO_INTERFACE.0
                } else if !broker.connected {
                    0x80010108
                } else {
                    let owner = self.active_rpc_invoke(index, thread);
                    self.ole32.as_mut().unwrap().allocate_rpc_buffer(
                        self.memory.as_mut().unwrap(),
                        index,
                        args[1],
                    )?;
                    let runtime = self.ole32.as_mut().unwrap();
                    runtime.rpc.buffers.last_mut().unwrap().owner = owner;
                    0
                }
            }
            4 => {
                if !broker.connected {
                    0x80010108
                } else {
                    self.begin_rpc_invoke(owner, token, frame, index)?;
                    return Ok(true);
                }
            }
            5 => {
                let owner = self.active_rpc_invoke(index, thread);
                let outcome = self.ole32.as_mut().unwrap().free_rpc_buffer_for(
                    self.memory.as_mut().unwrap(),
                    index,
                    args[1],
                    owner,
                );
                match outcome {
                    Ok(()) => 0,
                    Err(HostError::InvalidRequest) => HResult32::INVALID_ARGUMENT.0,
                    Err(error) => return Err(error),
                }
            }
            6 => {
                self.memory
                    .as_mut()
                    .unwrap()
                    .write_spans(&[
                        (GuestAddress(args[1]), &3u32.to_le_bytes()),
                        (GuestAddress(args[2]), &0u32.to_le_bytes()),
                    ])
                    .map_err(HostError::Memory)?;
                0
            }
            7 => u32::from(!broker.connected),
            _ => return Err(HostError::InvalidRequest),
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
        Ok(true)
    }
    pub(super) fn reserve_rpc_call(&self) -> Result<u64, HostError> {
        let runtime = self.ole32.as_ref().ok_or(HostError::InvalidRequest)?;
        if runtime.rpc.calls.len() == runtime.rpc.calls.capacity() {
            return Err(HostError::Infrastructure);
        }
        runtime
            .activation
            .next
            .checked_add(1)
            .ok_or(HostError::Infrastructure)
    }
    fn begin_rpc_invoke(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        broker: usize,
    ) -> Result<(), HostError> {
        let continuation = self.reserve_rpc_call()?;
        let args = frame.arguments();
        let message = read_message(self.memory()?, args[1])?;
        self.memory()?
            .check_span(GuestAddress(args[1]), 44, Access::Write)
            .map_err(HostError::Memory)?;
        if args[2] != 0 {
            self.memory()?
                .check_span(GuestAddress(args[2]), 4, Access::Write)
                .map_err(HostError::Memory)?;
        }
        let packet_owner = self.active_rpc_invoke(broker, self.threads.current_id());
        let runtime = self.ole32.as_ref().unwrap();
        let buffer = &runtime.rpc.buffers[runtime.rpc.packet(
            broker,
            message.buffer,
            message.length,
            packet_owner,
        )?];
        if buffer.owner.is_some()
            && runtime.rpc.calls.iter().any(|call| {
                call.phase == RpcPhase32::Invoke && call.request_lease == Some(buffer.id)
            })
        {
            return Err(HostError::InvalidRequest);
        }
        buffer
            .scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        let request_lease = buffer.id;
        let execution = runtime.rpc.brokers[broker].as_ref().unwrap().source;
        let channel = runtime.rpc.brokers[broker]
            .as_ref()
            .unwrap()
            .channel
            .ok_or(HostError::Infrastructure)?;
        let hold = runtime
            .com
            .prepare_lifetime_changes(self.memory()?, &[], &[], &[channel], &[], &[])
            .map_err(com_error)?;
        runtime
            .com
            .can_commit_lifetime_changes(self.memory()?, &hold)
            .map_err(com_error)?;
        let scratch = crate::process::windows::callbacks::GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            super::super::activation::ActivationRuntime32::scratch_reservation(),
            &message.encode(),
        )
        .map_err(HostError::Memory)?;
        self.ole32
            .as_mut()
            .unwrap()
            .com
            .commit_lifetime_after_aggregate_unmap(self.memory.as_ref().unwrap(), hold, true);
        let call = RpcCall32 {
            continuation,
            sender: self.threads.current_id(),
            owner,
            token,
            frame,
            broker,
            execution,
            remote: Remote32::Idle,
            phase: RpcPhase32::Invoke,
            message: args[1],
            status: args[2],
            scratch: Some(scratch),
            request_buffer: message.buffer,
            abandoned: false,
            cancelled: false,
            returned: 0,
            request: Some(message),
            request_lease: Some(request_lease),
            held_channel: Some(channel),
        };
        self.publish_rpc_call(call)?;
        Ok(())
    }
    fn begin_rpc_cleanup(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        broker: usize,
    ) -> Result<(), HostError> {
        let continuation = self.reserve_rpc_call()?;
        let runtime = self.ole32.as_ref().unwrap();
        let entry = runtime.rpc.brokers[broker].as_ref().unwrap();
        let pointer = entry.controller.unwrap();
        let execution = entry.client;
        let outcome = self
            .ole32
            .as_mut()
            .unwrap()
            .com
            .release(self.memory.as_mut().unwrap(), pointer)
            .map_err(com_error)?;
        assert!(outcome.destroyed.is_some());
        let entry = self.ole32.as_mut().unwrap().rpc.brokers[broker]
            .as_mut()
            .unwrap();
        entry.controller_live = false;
        entry.connected = false;
        let call = RpcCall32 {
            continuation,
            sender: self.threads.current_id(),
            owner,
            token,
            frame,
            broker,
            execution,
            remote: Remote32::Idle,
            phase: RpcPhase32::Cleanup(0),
            message: 0,
            status: 0,
            scratch: None,
            request_buffer: 0,
            abandoned: false,
            cancelled: false,
            returned: 0,
            request: None,
            request_lease: None,
            held_channel: None,
        };
        self.publish_rpc_call(call)
    }
    pub(super) fn publish_rpc_call(&mut self, call: RpcCall32) -> Result<(), HostError> {
        let runtime = self.ole32.as_mut().unwrap();
        runtime.activation.next = call.continuation;
        if call.phase == RpcPhase32::Invoke {
            let buffer = runtime
                .rpc
                .buffer(call.broker, call.request_buffer, 0)
                .expect("accepted invocation retained its validated request buffer");
            runtime.rpc.buffers[buffer].owner = Some(call.continuation);
        }
        runtime.rpc.calls.push(call);
        let index = runtime.rpc.calls.len() - 1;
        self.advance_rpc_call(index)
    }
}
