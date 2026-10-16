use super::{
    D3D8Config32, D3D8Error32, D3D8Runtime32, DispatchError32, resources::PrivateCallbackRequest32,
};
use crate::{
    graphics::d3d8::{
        api::{CREATE_GATE, HResult, InterfaceKind, MethodGate},
        commands::{CommandError, ReadbackId},
        device::DeviceError,
    },
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, HostError, WindowsCallback32, WindowsCallbackProvider32,
        WindowsProviderOutcome32, calls::call::PendingOwner,
    },
    windows::{CallFrame32, CallingConvention32},
};

#[derive(Clone, Copy)]
pub(in crate::process) struct D3D8Reply32 {
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) epoch: u64,
    pub(super) kind: ReplyKind32,
}
#[derive(Clone, Copy)]
pub(super) enum ReplyKind32 {
    Retry,
    Drain(u32),
    Readback(ReadbackId),
    LockReadback(ReadbackId),
    Ready(u32),
}
impl D3D8Reply32 {
    pub(in crate::process) fn ready(&self, runtime: &D3D8Runtime32) -> bool {
        let device = runtime.device_by_epoch(self.epoch).ok();
        match self.kind {
            ReplyKind32::Ready(_) => true,
            ReplyKind32::Retry | ReplyKind32::Drain(_) => device
                .is_none_or(|device| !device.queue().is_draining() && !device.queue().is_sealed()),
            ReplyKind32::Readback(id) | ReplyKind32::LockReadback(id) => {
                device.is_none_or(|device| device.pending_readback(id).is_none())
            }
        }
    }
}

impl EngineInstance {
    pub(in crate::process) fn d3d8_provider_owns_token(&self, token: u32) -> bool {
        self.d3d8_reply.is_some_and(|reply| reply.token == token)
    }
    pub fn configure_d3d8(&mut self, config: D3D8Config32) -> Result<(), HostError> {
        self.memory()?;
        if self.d3d8.is_some() {
            return Err(HostError::D3D8(D3D8Error32::AlreadyConfigured));
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.check_guest_reservation_admission(&[config.objects, config.locks])?;
        self.d3d8 = Some(D3D8Runtime32::new(config).map_err(HostError::D3D8)?);
        Ok(())
    }
    pub fn register_d3d8_module(&mut self, preferred_base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::graphics::d3d8::api::system_module(preferred_base))
    }
    pub(in crate::process) fn try_complete_d3d8_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let gate = MethodGate::from_id(id);
        if id != CREATE_GATE && gate.is_none() {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        let words = gate.map_or(1, |gate| u32::from(gate.method().stack_words));
        if frame.convention() != CallingConvention32::Stdcall || frame.stack_words() != words {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_callback_capacity()?;
        if let Some(reply) = self.d3d8_reply {
            if reply.owner != owner || reply.token != token {
                return Err(HostError::Call(CallError::Busy));
            }
            let runtime = self
                .d3d8
                .as_ref()
                .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
            let device = runtime.device_by_epoch(reply.epoch).ok();
            let result = match reply.kind {
                ReplyKind32::Ready(result) => Some(result),
                ReplyKind32::Drain(result)
                    if device.is_none_or(|device| {
                        !device.queue().is_draining() && !device.queue().is_sealed()
                    }) =>
                {
                    Some(result)
                }
                ReplyKind32::Readback(request)
                    if device.is_some_and(|device| device.pending_readback(request).is_none()) =>
                {
                    Some(0)
                }
                ReplyKind32::LockReadback(request)
                    if device.is_some_and(|device| device.pending_readback(request).is_none()) =>
                {
                    None
                }
                ReplyKind32::Retry => None,
                _ => return Err(HostError::Call(CallError::Busy)),
            };
            if let Some(result) = result {
                self.d3d8_reply = None;
                self.finish_d3d8_result(frame, result)?;
                return Ok(true);
            }
            self.d3d8_reply = None;
        }
        let gates = if id == CREATE_GATE {
            Some(self.module_gates()?)
        } else {
            None
        };
        let epoch = if gate
            .is_some_and(|gate| gate.interface == InterfaceKind::Direct3D && gate.slot == 15)
        {
            crate::process::media::next_session()?
        } else {
            0
        };
        let windows = self
            .user32
            .as_ref()
            .map(super::super::user32::User32Context32::windows);
        let runtime = self
            .d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        runtime.collect_garbage().map_err(dispatch_host_error)?;
        let device_epoch = gate
            .and_then(|_| runtime.object(GuestAddress(frame.arguments()[0])).ok())
            .and_then(|object| match &object.kind {
                super::objects::ObjectKind32::Device(device) => {
                    Some(device.backend.queue().epoch())
                }
                super::objects::ObjectKind32::Resource(resource) => runtime
                    .device(resource.device)
                    .ok()
                    .map(|device| device.queue().epoch()),
                super::objects::ObjectKind32::View(view) => runtime
                    .device(view.device)
                    .ok()
                    .map(|device| device.queue().epoch()),
                super::objects::ObjectKind32::SwapChain(chain) => runtime
                    .device(chain.device)
                    .ok()
                    .map(|device| device.queue().epoch()),
                _ => None,
            });
        if device_epoch.is_some_and(|epoch| {
            runtime
                .device_by_epoch(epoch)
                .is_ok_and(|device| device.queue().is_draining() || device.queue().is_sealed())
        }) {
            self.d3d8_reply = Some(D3D8Reply32 {
                owner,
                token,
                epoch: device_epoch.unwrap(),
                kind: ReplyKind32::Retry,
            });
            return Err(HostError::Call(CallError::Busy));
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = if id == CREATE_GATE {
            Ok(runtime
                .create_direct3d(memory, frame.arguments()[0], gates.as_deref().unwrap())
                .unwrap_or(0))
        } else {
            runtime.method_call(memory, windows, gate.unwrap(), frame.arguments(), epoch)
        };
        match result {
            Ok(result) => {
                if !runtime.deferred_private_releases.is_empty() {
                    let pointers = std::mem::take(&mut runtime.deferred_private_releases);
                    match runtime.release_private_callbacks(memory, pointers, result) {
                        Ok(value) => {
                            self.finish_d3d8_result(frame, value)?;
                        }
                        Err(DispatchError32::Callback(request)) => {
                            self.park_d3d8_callback(callback(request))?;
                        }
                        Err(error) => {
                            self.finish_d3d8_result(frame, dispatch_hresult(error).0)?;
                        }
                    }
                } else if device_epoch.is_some_and(|epoch| {
                    runtime
                        .device_by_epoch(epoch)
                        .is_ok_and(|device| device.queue().is_draining())
                }) {
                    self.d3d8_reply = Some(D3D8Reply32 {
                        owner,
                        token,
                        epoch: device_epoch.unwrap(),
                        kind: ReplyKind32::Drain(result),
                    });
                    return Err(HostError::Call(CallError::Busy));
                } else {
                    self.finish_d3d8_result(frame, result)?;
                }
            }
            Err(DispatchError32::Callback(request)) => {
                self.park_d3d8_callback(callback(request))?
            }
            Err(DispatchError32::Readback { id }) => {
                self.d3d8_reply = Some(D3D8Reply32 {
                    owner,
                    token,
                    epoch: id.epoch,
                    kind: ReplyKind32::Readback(id),
                });
                return Err(HostError::Call(CallError::Busy));
            }
            Err(DispatchError32::LockReadback { id }) => {
                self.d3d8_reply = Some(D3D8Reply32 {
                    owner,
                    token,
                    epoch: id.epoch,
                    kind: ReplyKind32::LockReadback(id),
                });
                return Err(HostError::Call(CallError::Busy));
            }
            Err(DispatchError32::Device(DeviceError::Command(CommandError::Backpressure))) => {
                let epoch = device_epoch.ok_or(HostError::Infrastructure)?;
                self.d3d8_reply = Some(D3D8Reply32 {
                    owner,
                    token,
                    epoch,
                    kind: ReplyKind32::Retry,
                });
                return Err(HostError::Call(CallError::Busy));
            }
            Err(error) => {
                let result = dispatch_hresult(error).0;
                if runtime.deferred_private_releases.is_empty() {
                    self.finish_d3d8_result(frame, result)?;
                } else {
                    let pointers = std::mem::take(&mut runtime.deferred_private_releases);
                    match runtime.release_private_callbacks(memory, pointers, result) {
                        Ok(value) => self.finish_d3d8_result(frame, value)?,
                        Err(DispatchError32::Callback(request)) => {
                            self.park_d3d8_callback(callback(request))?
                        }
                        Err(error) => self.finish_d3d8_result(frame, dispatch_hresult(error).0)?,
                    }
                }
            }
        }
        Ok(true)
    }
    fn finish_d3d8_result(&mut self, frame: CallFrame32, result: u32) -> Result<(), HostError> {
        let prepared = Self::prepare_completed_call(frame.complete(result))?;
        self.publish_prepared_call(prepared);
        Ok(())
    }
    pub(in crate::process) fn resume_d3d8_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let runtime = self
            .d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let resumed = runtime.resume_private_callback(continuation, result, memory);
        let resumed = match resumed {
            Err(DispatchError32::Callback(request)) => {
                return Ok(WindowsProviderOutcome32::Callback(callback(request)));
            }
            Ok(value) => value,
            Err(error) => dispatch_hresult(error).0,
        };
        let finished = if runtime.deferred_private_releases.is_empty() {
            Ok(resumed)
        } else {
            let pointers = std::mem::take(&mut runtime.deferred_private_releases);
            runtime.release_private_callbacks(memory, pointers, resumed)
        };
        match finished {
            Ok(value) => Ok(WindowsProviderOutcome32::Return(value)),
            Err(DispatchError32::Callback(request)) => {
                Ok(WindowsProviderOutcome32::Callback(callback(request)))
            }
            Err(error) => Ok(WindowsProviderOutcome32::Return(dispatch_hresult(error).0)),
        }
    }
}

fn callback(request: PrivateCallbackRequest32) -> WindowsCallback32 {
    let mut arguments = [0; 16];
    arguments[..4].copy_from_slice(&request.arguments);
    WindowsCallback32 {
        procedure: request.procedure,
        arguments,
        count: u32::from(request.argument_count),
        continuation: request.continuation,
        provider: WindowsCallbackProvider32::D3D8,
    }
}
fn dispatch_hresult(error: DispatchError32) -> HResult {
    match error {
        DispatchError32::Api(value) => value,
        DispatchError32::Device(value) => value.into(),
        _ => HResult::INVALID_CALL,
    }
}
fn dispatch_host_error(error: DispatchError32) -> HostError {
    match error {
        DispatchError32::Device(error) => HostError::D3D8(D3D8Error32::Device(error)),
        _ => HostError::Infrastructure,
    }
}
