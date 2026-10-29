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
    pub(in crate::process) fn try_inline_external_cpu_gate_with(
        &mut self,
        pc: u32,
        complete: impl FnOnce(&mut Self, u32) -> Result<(), HostError>,
    ) -> Result<bool, HostError> {
        use crate::abi::{arena::*, x86::*};
        use crate::cpu::{ExecutionExit, ExitReason};
        if self.pending_call.is_some()
            || self.pending_windows_thread_schedule().is_some()
            || self.pending_windows_callback().is_some()
        {
            return Ok(false);
        }
        let Some((id, true)) = self.external_cpu_gate_match(pc) else {
            return Ok(false);
        };
        if !matches!(
            id,
            0x10001 | 0x10002 | 0xf000b | 0x40225 | 0x4022a | 0x4022e | 0x40232 | 0x4023f
        ) || !self.external_cpu_can_defer_gate(pc, id)
        {
            return Ok(false);
        }
        let arena = self.arena();
        let state = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let x87 = decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let simd = decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let ymm = decode_ymm(&arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let segments = self.segment_context()?;
        let prepared = self.prepare_core_exit(
            state,
            ExecutionExit {
                retired: 0,
                reason: ExitReason::Gate { id },
            },
            0,
            None,
            None,
            (x87, simd, ymm, segments),
        )?;
        self.commit_core_exit(prepared);
        // after publication, every unexpected outcome is terminal to the inline caller.
        if self.try_complete_external_cpu_gate_with(complete) != Ok(true)
            || self.guard_core_entry().is_err()
            || self.callback.is_some()
            || self.pending_windows_callback().is_some()
            || !matches!(
                decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]),
                Ok(ExecutionExit {
                    retired: 0,
                    reason: ExitReason::NeedCode
                })
            )
        {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(true)
    }

    #[cfg(all(feature = "external-cpu", test))]
    pub(in crate::process) fn try_complete_external_cpu_gate(&mut self) -> Result<bool, HostError> {
        self.try_complete_external_cpu_gate_with(|instance, token| {
            instance.complete_core_windows_call(token)
        })
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn try_complete_external_cpu_gate_with(
        &mut self,
        complete: impl FnOnce(&mut Self, u32) -> Result<(), HostError>,
    ) -> Result<bool, HostError> {
        use crate::abi::{arena::*, call_frame::CALL_FRAME_SIZE, x86::*};
        use crate::cpu::ExitReason;
        if self.pending_call.is_some() {
            return Err(HostError::InvalidRequest);
        }
        if !self.external_cpu_memory_pinned || self.guard_core_entry().is_err() {
            return Ok(false);
        }
        let arena = self.arena();
        let state = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Ok(false);
        };
        if !self
            .external_cpu_gate_match(state.eip)
            .is_some_and(|(gate, _)| gate == id)
            || crate::process::windows_gate_info(id).is_none()
        {
            return Ok(false);
        }
        let record = self.capture_core_windows_call()?;
        let captured: [u8; CALL_FRAME_SIZE] = self.arena()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE]
            .try_into()
            .unwrap();
        let result = complete(self, record.token);
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE]
            .copy_from_slice(&captured);
        result.map(|()| true)
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_cpu_can_defer_gate(&self, pc: u32, id: u32) -> bool {
        self.external_cpu_deferred_frame(pc, id, None).is_some()
    }

    #[cfg(feature = "external-cpu")]
    #[inline(always)]
    pub(super) fn external_cpu_deferred_frame(
        &self,
        pc: u32,
        id: u32,
        local_state: Option<crate::cpu::x86::State32>,
    ) -> Option<crate::windows::CallFrame32> {
        use crate::{
            abi::{
                arena::STATE_OFFSET,
                x86::{STATE_SIZE, decode_state},
            },
            graphics::d3d8::api::{InterfaceKind, MethodGate},
            memory::{Access, GuestAddress},
            windows::{CallFrame32, CallingConvention32, WindowsApi32, thread::ThreadApi32},
        };
        let getter = id == WindowsApi32::GetLastError.id();
        let setter = id == WindowsApi32::SetLastError.id();
        let tls_getter = id == ThreadApi32::TlsGetValue.id();
        let gate = MethodGate::from_id(id);
        if !getter
            && !setter
            && !tls_getter
            && !gate.is_some_and(|gate| {
                gate.interface == InterfaceKind::Device
                    && (matches!(gate.slot, 37 | 42 | 46 | 50 | 61 | 63 | 70 | 71 | 83 | 85)
                        || (local_state.is_some() && matches!(gate.slot, 40 | 44)))
            })
        {
            return None;
        }
        if !self.external_cpu_memory_pinned
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
            return None;
        }
        let state = match local_state {
            Some(state) => state,
            None => decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).ok()?,
        };
        if state.eip != pc {
            return None;
        }
        let Ok(memory) = self.memory() else {
            return None;
        };
        let Ok(frame) = CallFrame32::capture(
            memory,
            state,
            CallingConvention32::Stdcall,
            if setter || tls_getter {
                1
            } else {
                gate.map_or(0, |gate| u32::from(gate.method().stack_words))
            },
        ) else {
            return None;
        };
        if Self::prepare_completed_call(frame.complete(0)).is_err() {
            return None;
        }
        if getter {
            // getlasterror republishes the same TEB value, so no guest RAM write occurs.
            return self
                .module_teb_address()
                .is_none_or(|teb| {
                    teb.checked_add(0x34).is_some_and(|address| {
                        memory
                            .check_span(GuestAddress(address), 4, Access::Read)
                            .is_ok()
                    })
                })
                .then_some(frame);
        }
        if setter || tls_getter {
            let argument = frame.arguments()[0];
            if tls_getter {
                if argument >= 1088 {
                    return None;
                }
                if self.threads.current_teb().is_some_and(|teb| {
                    let offset = if argument < 64 {
                        0xe10 + argument * 4
                    } else {
                        0xf94
                    };
                    teb.checked_add(offset).is_none()
                }) || self.get_windows_tls(argument).is_err()
                {
                    return None;
                }
            }
            let target = if setter { argument } else { 0 };
            return self
                .module_teb_address()
                .is_none_or(|teb| {
                    let Some(address) = teb.checked_add(0x34).map(GuestAddress) else {
                        return false;
                    };
                    let mut current = [0; 4];
                    if memory.read(address, &mut current).is_err() {
                        return false;
                    }
                    if setter && u32::from_le_bytes(current) == target {
                        return memory.check_span(address, 4, Access::Write).is_ok();
                    }
                    memory.external_write_is_stable(address, 4)
                        && memory.ensure_transaction_versions(1).is_ok()
                })
                .then_some(frame);
        }
        let gate = gate.unwrap();
        let args = frame.arguments();
        let input = match gate.slot {
            37 => Some((args[2], 64)),
            40 => Some((args[1], 24)),
            42 => Some((args[1], 68)),
            44 => Some((args[2], 104)),
            _ => None,
        };
        if input.is_some_and(|(address, length)| {
            if matches!(gate.slot, 40 | 44) {
                let Some(end) = u64::from(address)
                    .checked_add(length as u64)
                    .filter(|&end| address != 0 && end <= 1 << 32)
                else {
                    return true;
                };
                // declining a direct input must not arm a guarded service fault.
                return (u64::from(address) / 4096..=(end - 1) / 4096).any(|page| {
                    !memory
                        .permissions_at(GuestAddress((page * 4096) as u32))
                        .is_ok_and(|permissions| permissions.allows(Access::Read))
                });
            }
            memory
                .check_span(GuestAddress(address), length, Access::Read)
                .is_err()
        }) {
            return None;
        }
        self.d3d8
            .as_ref()
            .is_some_and(|runtime| {
                if self.external_graphics_overlap_gate_safe(gate, args) {
                    return true;
                }
                if matches!(gate.slot, 61 | 83 | 85) {
                    runtime.external_cpu_binding_gate_safe(
                        memory,
                        GuestAddress(args[0]),
                        gate.slot,
                        args,
                    )
                } else {
                    runtime.external_cpu_scalar_gate_safe(memory, GuestAddress(args[0]))
                }
            })
            .then_some(frame)
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
