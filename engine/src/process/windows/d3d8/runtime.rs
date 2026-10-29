use super::{
    D3D8Config32, D3D8Error32, D3D8Runtime32, DispatchError32,
    fpu::{FpuSetup32, PreparedFpu32},
    resources::PrivateCallbackRequest32,
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
    pub(super) window: Option<super::super::user32::User32Callback32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct D3D8ReadbackOwner32 {
    pub thread: u32,
    pub owner: PendingOwner,
    pub token: u32,
}
#[derive(Clone, Copy)]
pub(super) enum ReplyKind32 {
    Retry,
    Drain(u32),
    Readback(ReadbackId),
    LockReadback(ReadbackId),
    Ready(u32),
    Private,
    Window,
}
impl D3D8Reply32 {
    pub(in crate::process) fn readback_owner(&self, id: ReadbackId) -> Option<(PendingOwner, u32)> {
        (self.epoch == id.epoch
            && matches!(self.kind, ReplyKind32::Readback(request) | ReplyKind32::LockReadback(request) if request == id))
            .then_some((self.owner, self.token))
    }

    pub(in crate::process) fn fail_readback(
        &mut self,
        epoch: u64,
        id: Option<ReadbackId>,
        result: u32,
    ) {
        if self.epoch == epoch
            && matches!(self.kind, ReplyKind32::Readback(request) | ReplyKind32::LockReadback(request)
                if id.is_none_or(|id| id == request))
        {
            self.kind = ReplyKind32::Ready(result);
        }
    }

    pub(in crate::process) fn ready(&self, runtime: &D3D8Runtime32) -> bool {
        let device = runtime.device_by_epoch(self.epoch).ok();
        match self.kind {
            ReplyKind32::Private | ReplyKind32::Window => false,
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
    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn set_external_graphics_overlap(
        &mut self,
        enabled: u32,
    ) -> Result<(), HostError> {
        if enabled > 1 || !self.external_cpu_memory_pinned {
            return Err(HostError::InvalidRequest);
        }
        self.guard_core_owner(self.core_owner.ok_or(HostError::InvalidArtifact)?)?;
        self.guard_owned_host_mutation()?;
        self.external_graphics_overlap_enabled = enabled != 0;
        Ok(())
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_graphics_overlap(&self) -> [u32; 4] {
        [
            u32::from(self.external_graphics_overlap_enabled),
            0,
            self.external_graphics_overlap_completed_calls as u32,
            (self.external_graphics_overlap_completed_calls >> 32) as u32,
        ]
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_graphics_overlap_gate_safe(
        &self,
        gate: MethodGate,
        args: &[u32],
    ) -> bool {
        self.external_graphics_overlap_enabled
            && self.external_cpu_memory_pinned
            && self.external_graphics_overlap_completed_calls != u64::MAX
            && self.callback.is_none()
            && !self.has_active_module_operation()
            && self.image_input.is_none()
            && self.execution_frame.is_none()
            && !self.exceptions.has_deliveries()
            && self.pending_windows_thread_schedule().is_none()
            && self.windows_replies.is_empty()
            && self.module_windows_reply.is_none()
            && self.d3d8.as_ref().is_some_and(|runtime| {
                self.memory.as_ref().is_some_and(|memory| {
                    runtime.external_cpu_overlap_gate_safe(memory, gate, args)
                })
            })
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn execute_external_direct_d3d8_gate(
        &mut self,
        id: u32,
        frame: CallFrame32,
    ) -> Result<u32, HostError> {
        let Some(gate) = MethodGate::from_id(id).filter(|gate| {
            gate.interface == InterfaceKind::Device
                && matches!(gate.slot, 37 | 40 | 42 | 44 | 46 | 50 | 61 | 63 | 83 | 85)
        }) else {
            return Err(HostError::Call(CallError::InvalidRequest));
        };
        let overlap = self.external_graphics_overlap_gate_safe(gate, frame.arguments());
        let windows = self
            .user32
            .as_ref()
            .map(super::super::user32::User32Context32::windows);
        let runtime = self
            .d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let mut setup = FpuSetup32::NotReached;
        let result = runtime.method_call(memory, windows, gate, frame.arguments(), 0, &mut setup);
        if setup != FpuSetup32::NotReached {
            return Err(HostError::Infrastructure);
        }
        let completed = match result {
            Ok(value) => Ok(value),
            Err(error @ (DispatchError32::Api(_) | DispatchError32::Device(_))) => {
                Ok(dispatch_hresult(error).0)
            }
            Err(_) => Err(HostError::Infrastructure),
        }?;
        if overlap {
            self.external_graphics_overlap_completed_calls += 1;
        }
        Ok(completed)
    }

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
                self.finish_d3d8_result(frame, result)?;
                return Ok(true);
            }
            self.d3d8_reply = None;
        }
        let fpu = PreparedFpu32::prepare(self, gate, frame.arguments())?;
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
        #[cfg(feature = "external-cpu")]
        let overlap = gate
            .is_some_and(|gate| self.external_graphics_overlap_gate_safe(gate, frame.arguments()));
        #[cfg(not(feature = "external-cpu"))]
        let overlap = false;
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
            runtime.device_by_epoch(epoch).is_ok_and(|device| {
                device.queue().is_draining() || (device.queue().is_sealed() && !overlap)
            })
        }) {
            self.d3d8_reply = Some(D3D8Reply32 {
                owner,
                token,
                epoch: device_epoch.unwrap(),
                kind: ReplyKind32::Retry,
                window: None,
            });
            return Err(HostError::Call(CallError::Busy));
        }
        let window = match self.prepare_d3d8_window(gate, frame.arguments()) {
            Ok(window) => window,
            Err(HostError::User32(error)) => {
                use crate::{memory::MemoryError as M, windows::user32::User32Error32 as U};
                let result = match error {
                    U::Capacity | U::Allocation | U::Memory(M::Capacity | M::Allocation) => {
                        HResult::OUT_OF_MEMORY
                    }
                    U::InvalidRequest | U::Win32(_) => HResult::INVALID_CALL,
                    _ => return Err(HostError::User32(error)),
                };
                self.finish_d3d8_result(frame, result.0)?;
                return Ok(true);
            }
            Err(error) => return Err(error),
        };
        if window.is_some() {
            self.d3d8_reply = Some(D3D8Reply32 {
                owner,
                token,
                epoch: device_epoch.unwrap_or(epoch),
                kind: ReplyKind32::Private,
                window,
            });
        }
        let mut setup = FpuSetup32::NotReached;
        let result = {
            let runtime = self.d3d8.as_mut().unwrap();
            let windows = self
                .user32
                .as_ref()
                .map(super::super::user32::User32Context32::windows);
            let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
            if id == CREATE_GATE {
                Ok(runtime
                    .create_direct3d(memory, frame.arguments()[0], gates.as_deref().unwrap())
                    .unwrap_or(0))
            } else {
                runtime.method_call(
                    memory,
                    windows,
                    gate.unwrap(),
                    frame.arguments(),
                    epoch,
                    &mut setup,
                )
            }
        };
        if setup == FpuSetup32::Entered {
            fpu.unwrap().commit(self);
        }
        let runtime = self.d3d8.as_mut().unwrap();
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
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
                        window,
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
                    window: None,
                });
                return Err(HostError::Call(CallError::Busy));
            }
            Err(DispatchError32::LockReadback { id }) => {
                self.d3d8_reply = Some(D3D8Reply32 {
                    owner,
                    token,
                    epoch: id.epoch,
                    kind: ReplyKind32::LockReadback(id),
                    window: None,
                });
                return Err(HostError::Call(CallError::Busy));
            }
            Err(DispatchError32::Device(DeviceError::Command(CommandError::Backpressure))) => {
                let epoch = device_epoch.ok_or(HostError::Infrastructure)?;
                self.cancel_d3d8_window()?;
                self.d3d8_reply = Some(D3D8Reply32 {
                    owner,
                    token,
                    epoch,
                    kind: ReplyKind32::Retry,
                    window: None,
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
        #[cfg(feature = "external-cpu")]
        if overlap && self.pending_call.is_none() {
            self.external_graphics_overlap_completed_calls += 1;
        }
        Ok(true)
    }
    fn finish_d3d8_result(&mut self, frame: CallFrame32, result: u32) -> Result<(), HostError> {
        let prepared = Self::prepare_completed_call(frame.complete(result))?;
        match self.d3d8_window_outcome(result)? {
            WindowsProviderOutcome32::Callback(request) => self.park_d3d8_callback(request)?,
            WindowsProviderOutcome32::Return(_) => {
                self.d3d8_reply = None;
                self.publish_prepared_call(prepared);
            }
        }
        Ok(())
    }
    pub(in crate::process) fn resume_d3d8_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        if self
            .d3d8_reply
            .is_some_and(|reply| matches!(reply.kind, ReplyKind32::Window))
        {
            return self.resume_d3d8_window(continuation, result);
        }
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
            Ok(value) => self.d3d8_window_outcome(value),
            Err(DispatchError32::Callback(request)) => {
                Ok(WindowsProviderOutcome32::Callback(callback(request)))
            }
            Err(error) => self.d3d8_window_outcome(dispatch_hresult(error).0),
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
