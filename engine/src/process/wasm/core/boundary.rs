use super::{CoreSession32, NumericBank, REGISTRY, Registry, status};
use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{STATE_SIZE, decode_state},
    },
    cpu::{
        ExecutionExit, ExitReason, InfrastructureFailure, SystemEntryKind32, UnsupportedFeature,
        x86::{DebugCause32, EventOrigin32, ExceptionCause32, SelectorErrorCode32},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{CpuInstructionSite32, HostError},
};

pub(crate) fn core_numeric(context: u32, ordinal: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 8;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else { return 8 };
        if !core.active || core.context_address() as u32 != context {
            return 8;
        }
        let Some(instruction) = core
            .code
            .as_ref()
            .and_then(|code| code.native_instructions().get(ordinal as usize).copied())
        else {
            return 8;
        };
        let Some(instance) = instance.as_mut() else {
            return 8;
        };
        if let Some(code) = core.code.as_ref() {
            core.tracing.numeric(code, instruction.pc, instruction.next);
        }
        core.numeric
            .execute(instance, &mut core.context, &instruction)
    })
}

pub(crate) fn core_import() -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else { return 7 };
        if core.active {
            return 12;
        }
        let Some(instance) = instance.as_mut() else {
            return 5;
        };
        if let Err(error) = instance.guard_core_entry() {
            return status(error);
        }
        let state = match decode_state(&instance.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]) {
            Ok(state) => state,
            Err(_) => return 7,
        };
        let bank = match NumericBank::from_instance(instance) {
            Ok(bank) => bank,
            Err(error) => return status(error),
        };
        core.context.set_state(state);
        for offset in [48, 52, 116] {
            core.context.set_word(offset, 0);
        }
        *core.numeric = bank;
        core.exported_retired = core.context.retired();
        0
    })
}

fn word(core: &CoreSession32, offset: usize) -> u32 {
    u32::from_le_bytes(core.context.bytes()[offset..offset + 4].try_into().unwrap())
}

fn exit_reason(
    core: &CoreSession32,
    stop: u32,
) -> Result<(ExitReason, Option<EventOrigin32>), HostError> {
    let detail = word(core, 48);
    let vector = word(core, 116);
    let cause = match (stop, vector) {
        (9, 0) => Some(ExceptionCause32::DivideError),
        (9, 1) => Some(ExceptionCause32::Debug(
            DebugCause32::InstructionBreakpointFault,
        )),
        (9, 3) => Some(ExceptionCause32::BreakpointInt3),
        (9, 4) => Some(ExceptionCause32::OverflowInto),
        (9, 5) => Some(ExceptionCause32::BoundRange),
        (9, 6) => Some(ExceptionCause32::InvalidOpcode),
        (9, 7) => Some(ExceptionCause32::DeviceNotAvailable),
        (9, 10..=13) => {
            let code = SelectorErrorCode32::from_bits(detail).ok_or(HostError::InvalidRequest)?;
            Some(match vector {
                10 => ExceptionCause32::InvalidTss(code),
                11 => ExceptionCause32::SegmentNotPresent(code),
                12 => ExceptionCause32::StackSegment(code),
                _ => ExceptionCause32::GeneralProtection(code),
            })
        }
        (9, 16) => Some(ExceptionCause32::FloatingPoint {
            status: detail as u16,
        }),
        (9, 19) => Some(ExceptionCause32::SimdFloatingPoint { mxcsr: detail }),
        _ => None,
    };
    if let Some(cause) = cause {
        let reason = match cause {
            ExceptionCause32::DivideError => ExitReason::DivideError,
            ExceptionCause32::Debug(_) => ExitReason::DebugTrap,
            ExceptionCause32::BreakpointInt3 => ExitReason::Breakpoint,
            ExceptionCause32::OverflowInto => ExitReason::Overflow,
            ExceptionCause32::BoundRange => ExitReason::BoundRange,
            ExceptionCause32::InvalidOpcode => ExitReason::InvalidOpcode,
            ExceptionCause32::DeviceNotAvailable => ExitReason::DeviceNotAvailable,
            ExceptionCause32::InvalidTss(code) => ExitReason::InvalidTss {
                error_code: code.get(),
            },
            ExceptionCause32::SegmentNotPresent(code) => ExitReason::SegmentNotPresent {
                error_code: code.get(),
            },
            ExceptionCause32::StackSegment(code) => ExitReason::StackSegment {
                error_code: code.get(),
            },
            ExceptionCause32::GeneralProtection(code) => ExitReason::GeneralProtection {
                error_code: code.get(),
            },
            ExceptionCause32::FloatingPoint { status } => ExitReason::FloatingPoint { status },
            ExceptionCause32::SimdFloatingPoint { mxcsr } => {
                ExitReason::SimdFloatingPoint { mxcsr }
            }
            ExceptionCause32::PageFault(_) => unreachable!(),
        };
        return Ok((reason, Some(EventOrigin32::Exception(cause))));
    }
    let reason = match stop {
        1 => ExitReason::NeedCode,
        2 => ExitReason::Budget,
        3 => {
            let metadata = word(core, 52);
            let width = if vector >> 8 != 0 {
                vector >> 8
            } else {
                metadata & 255
            };
            let access = match metadata >> 8 & 255 {
                1 => Access::Read,
                2 => Access::Write,
                4 => Access::Execute,
                _ => return Err(HostError::InvalidRequest),
            };
            let reason = match metadata >> 16 {
                1 => FaultReason::Unmapped,
                2 => FaultReason::Permission,
                3 => FaultReason::AddressOverflow,
                _ => return Err(HostError::InvalidRequest),
            };
            ExitReason::MemoryFault {
                fault: MemoryFault {
                    address: GuestAddress(detail),
                    access,
                    reason,
                },
                length: width,
            }
        }
        4 | 5 => ExitReason::Unsupported(UnsupportedFeature::Opcode),
        6 => ExitReason::CodeInvalidated,
        7 => ExitReason::Gate { id: vector },
        8 => ExitReason::Infrastructure(InfrastructureFailure::HelperRejected),
        10 => {
            let kind = if vector == 0x100 {
                SystemEntryKind32::Sysenter
            } else if vector <= 255 {
                SystemEntryKind32::Interrupt(vector as u8)
            } else {
                return Err(HostError::InvalidRequest);
            };
            return Ok((
                ExitReason::SystemEntry { kind },
                Some(EventOrigin32::SystemEntry(kind)),
            ));
        }
        _ => return Err(HostError::InvalidRequest),
    };
    Ok((reason, None))
}

pub(crate) fn core_export(stop: u32, work: u32) -> u32 {
    REGISTRY.with(|registry| {
        let Ok(mut registry) = registry.try_borrow_mut() else {
            return 9;
        };
        let Registry { instance, core, .. } = &mut *registry;
        let Some(core) = core.as_mut() else { return 7 };
        if core.active {
            return 12;
        }
        let Some(instance) = instance.as_mut() else {
            return 5;
        };
        let (reason, origin) = match exit_reason(core, stop) {
            Ok(value) => value,
            Err(error) => return status(error),
        };
        let state = core.context.state();
        if let ExitReason::Gate { id } = reason {
            if !instance
                .module_gate_at(state.eip)
                .is_some_and(|gate| gate.id == id)
            {
                return 7;
            }
            let mut stub = [0; 2];
            if instance
                .memory
                .as_ref()
                .unwrap()
                .fetch(GuestAddress(state.eip), &mut stub)
                .is_err()
                || stub != [0x0f, 0x0b]
            {
                return 7;
            }
        }
        let retired = core.context.retired().wrapping_sub(core.exported_retired);
        if retired > work {
            return 7;
        }
        let instruction = (stop == 10).then(|| CpuInstructionSite32 {
            eip: word(core, 48),
            retired: retired.saturating_sub(1),
            flags: state.eflags,
            ss_debug_shadow: false,
            work_consumed: Some(work.saturating_sub(1)),
        });
        let prepared = match instance.prepare_core_exit(
            state,
            ExecutionExit { retired, reason },
            work,
            origin,
            instruction,
            core.numeric.architectural_state(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => return status(error),
        };
        if let Err(error) = core.numeric.publish_to_instance(instance) {
            return status(error);
        }
        instance.commit_core_exit(prepared);
        core.exported_retired = core.context.retired();
        0
    })
}

pub(crate) fn core_capture_windows_call() -> u32 {
    super::super::mutate(|instance| instance.capture_core_windows_call().map(|_| ()))
}

pub(crate) fn core_complete_windows_call(token: u32) -> u32 {
    super::super::mutate(|instance| instance.complete_core_windows_call(token))
}

pub(crate) fn core_begin_windows_callback(return_pc: u32, return_id: u32) -> u32 {
    super::super::mutate(|instance| {
        instance
            .begin_core_windows_callback(return_pc, return_id)
            .map(|_| ())
    })
}

pub(crate) fn core_finish_windows_callback(token: u32) -> u32 {
    super::super::mutate(|instance| instance.finish_core_windows_callback(token).map(|_| ()))
}

pub(crate) fn core_dispatch_exception() -> u32 {
    let identity = REGISTRY.with(|registry| {
        registry.try_borrow().ok().and_then(|registry| {
            let instance = registry.instance.as_ref()?;
            Some((instance.key, instance.cpu_event.as_ref()?.epoch))
        })
    });
    let Some((key, epoch)) = identity else {
        return 7;
    };
    super::super::dispatch_windows_exception(
        key as u32,
        (key >> 32) as u32,
        epoch as u32,
        (epoch >> 32) as u32,
    )
}

pub(crate) fn core_finish_exception() -> u32 {
    let identity = REGISTRY.with(|registry| {
        registry.try_borrow().ok().and_then(|registry| {
            let instance = registry.instance.as_ref()?;
            Some((instance.key, instance.pending_windows_exception()?.id))
        })
    });
    let Some((key, id)) = identity else { return 7 };
    super::super::finish_windows_exception_handler(
        key as u32,
        (key >> 32) as u32,
        id as u32,
        (id >> 32) as u32,
    )
}
