use std::sync::atomic::{AtomicU64, Ordering};

use crate::process::{CallError, EngineInstance, HostError, calls::call::PendingOwner};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

pub(in crate::process) struct PreparedCoreExit32 {
    state: [u8; crate::abi::x86::STATE_SIZE],
    exit: [u8; crate::abi::x86::EXIT_SIZE],
    work: u32,
    event: Option<crate::process::CpuEvent32>,
    epoch: u64,
}

impl EngineInstance {
    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_cpu_can_defer_gate(&self, pc: u32, id: u32) -> bool {
        use crate::{
            abi::{
                arena::STATE_OFFSET,
                x86::{STATE_SIZE, decode_state},
            },
            graphics::d3d8::api::{InterfaceKind, MethodGate},
            memory::{Access, GuestAddress},
            windows::{CallFrame32, CallingConvention32},
        };
        let Some(gate) = MethodGate::from_id(id) else {
            return false;
        };
        if gate.interface != InterfaceKind::Device
            || !matches!(gate.slot, 37 | 42 | 46 | 50 | 63)
            || !self.external_cpu_memory_pinned
            || self.guard_core_entry().is_err()
            || self.callback.is_some()
            || self.has_active_module_operation()
            || self.image_input.is_some()
            || self.execution_frame.is_some()
            || self.windows_service.is_some()
            || !self.windows_replies.is_empty()
            || self.module_windows_reply.is_some()
            || self.d3d8_reply.is_some()
        {
            return false;
        }
        let Ok(state) = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]) else {
            return false;
        };
        if state.eip != pc {
            return false;
        }
        let Ok(memory) = self.memory() else {
            return false;
        };
        let Ok(frame) = CallFrame32::capture(
            memory,
            state,
            CallingConvention32::Stdcall,
            u32::from(gate.method().stack_words),
        ) else {
            return false;
        };
        if Self::prepare_completed_call(frame.complete(0)).is_err() {
            return false;
        }
        let args = frame.arguments();
        let input = match gate.slot {
            37 => Some((args[2], 64)),
            42 => Some((args[1], 68)),
            _ => None,
        };
        if input.is_some_and(|(address, length)| {
            memory
                .check_span(GuestAddress(address), length, Access::Read)
                .is_err()
        }) {
            return false;
        }
        self.d3d8.as_ref().is_some_and(|runtime| {
            runtime.external_cpu_scalar_gate_safe(memory, GuestAddress(args[0]))
        })
    }

    pub(in crate::process) fn prepare_core_exit(
        &self,
        state: crate::cpu::x86::State32,
        exit: crate::cpu::ExecutionExit,
        work: u32,
        origin: Option<crate::cpu::x86::EventOrigin32>,
        instruction: Option<crate::process::CpuInstructionSite32>,
        banks: (
            crate::cpu::x86::X87State,
            crate::cpu::x86::simd::SimdState,
            crate::cpu::x86::simd::ymm::YmmUpperState,
            crate::cpu::x86::SegmentContext,
        ),
    ) -> Result<PreparedCoreExit32, HostError> {
        use crate::abi::x86::*;
        let owner = self.core_owner.ok_or(HostError::InvalidArtifact)?;
        self.guard_core_owner(owner)?;
        if self.pending_call.is_some() || self.cpu_event.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let epoch = self
            .core_epoch
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        let mut state_bytes = [0; STATE_SIZE];
        let mut exit_bytes = [0; EXIT_SIZE];
        encode_state_current(&state, &mut state_bytes).map_err(|_| HostError::InvalidRequest)?;
        encode_exit_v15(&exit, &mut exit_bytes).map_err(|_| HostError::InvalidRequest)?;
        let event = if origin.is_some()
            || matches!(exit.reason, crate::cpu::ExitReason::MemoryFault { .. })
        {
            Some(crate::process::CpuEvent32 {
                owner: PendingOwner::Core(owner),
                callback_token: self.callback.as_ref().map(|callback| callback.token()),
                epoch,
                origin,
                instruction,
                state,
                exit,
                ss_debug_shadow: false,
                x87: banks.0,
                simd: banks.1,
                ymm: banks.2,
                segments: banks.3,
                work_consumed: work,
                privileged_instruction: false,
                guard_page: match exit.reason {
                    crate::cpu::ExitReason::MemoryFault { fault, .. }
                        if fault.reason == crate::memory::FaultReason::Permission =>
                    {
                        self.memory()?.guard_page(fault.address)
                    }
                    _ => None,
                },
            })
        } else {
            None
        };
        Ok(PreparedCoreExit32 {
            state: state_bytes,
            exit: exit_bytes,
            work,
            event,
            epoch,
        })
    }

    pub(in crate::process) fn commit_core_exit(&mut self, prepared: PreparedCoreExit32) {
        use crate::abi::arena::*;
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + crate::abi::x86::STATE_SIZE]
            .copy_from_slice(&prepared.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE]
            .copy_from_slice(&prepared.exit);
        crate::abi::header::write(&mut arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE], *b"R3WK");
        crate::abi::header::write_u32(arena, WORK_OFFSET + 16, prepared.work);
        self.core_epoch = prepared.epoch;
        #[cfg(any(target_arch = "wasm32", test))]
        {
            self.detection_epoch = prepared.epoch;
        }
        self.cpu_event = prepared.event;
    }

    pub(in crate::process) fn enable_core_execution(&mut self) -> Result<u64, HostError> {
        self.memory()?;
        if let Some(id) = self.core_owner {
            return Ok(id);
        }
        if self.pending_call.is_some() || self.callback.is_some() || self.cpu_event.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let id = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| HostError::Infrastructure)?;
        self.core_owner = Some(id);
        Ok(id)
    }

    pub(in crate::process) fn guard_core_owner(&self, id: u64) -> Result<(), HostError> {
        self.memory()?;
        if id == 0 || self.core_owner != Some(id) {
            return Err(HostError::InvalidArtifact);
        }
        Ok(())
    }

    pub(in crate::process) fn guard_core_entry(&self) -> Result<(), HostError> {
        self.guard_core_owner(self.core_owner.ok_or(HostError::InvalidArtifact)?)?;
        self.guard_thread_execution_ready()?;
        if self.pending_call.is_some() || self.cpu_event.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if self
            .callback
            .as_ref()
            .is_some_and(|callback| !callback.is_core())
        {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub fn capture_core_windows_call(
        &mut self,
    ) -> Result<crate::abi::call_frame::CallRecord32, HostError> {
        let id = self.core_owner.ok_or(HostError::InvalidArtifact)?;
        self.guard_core_owner(id)?;
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let info = self.current_windows_gate_info()?;
        let convention = match info.convention {
            crate::windows::CallingConvention32::Cdecl => 1,
            crate::windows::CallingConvention32::Stdcall => 2,
            crate::windows::CallingConvention32::Thiscall => 3,
        };
        self.capture_call_frame(PendingOwner::Core(id), convention, info.stack_words)
    }

    pub fn complete_core_windows_call(&mut self, token: u32) -> Result<(), HostError> {
        let id = self.core_owner.ok_or(HostError::InvalidArtifact)?;
        self.guard_core_owner(id)?;
        self.complete_windows_call_frame(PendingOwner::Core(id), token)
    }

    pub fn begin_core_windows_callback(
        &mut self,
        return_pc: u32,
        return_id: u32,
    ) -> Result<crate::abi::callback::CallbackRecord32, HostError> {
        self.begin_windows_callback(self.key, 0, return_pc, return_id)
    }

    pub fn finish_core_windows_callback(
        &mut self,
        token: u32,
    ) -> Result<crate::abi::callback::CallbackRecord32, HostError> {
        self.finish_windows_callback(self.key, 0, token)
    }
}

#[cfg(test)]
mod tests;
