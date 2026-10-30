use crate::{
    cpu::{
        ExitReason, InfrastructureFailure, SystemEntryKind32, UnsupportedFeature,
        runtime::CpuState,
        x86::{DebugCause32, EventOrigin32, ExceptionCause32, SelectorErrorCode32},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{CpuInstructionSite32, HostError},
};

pub(super) fn decode(
    state: &CpuState,
    work: u32,
) -> Result<
    (
        ExitReason,
        Option<EventOrigin32>,
        Option<CpuInstructionSite32>,
    ),
    HostError,
> {
    if state.retired > work {
        return Err(HostError::InvalidRequest);
    }
    let result = match state.status {
        0 | 1 => (ExitReason::NeedCode, None, None),
        2 => (ExitReason::Budget, None, None),
        3 => {
            let width = if state.vector == 0 {
                state.auxiliary & 255
            } else {
                state.vector
            };
            if width == 0 || width > 832 || state.auxiliary & 255 != width & 255 {
                return Err(HostError::InvalidRequest);
            }
            let access = match state.auxiliary >> 8 & 255 {
                1 => Access::Read,
                2 => Access::Write,
                4 => Access::Execute,
                _ => return Err(HostError::InvalidRequest),
            };
            let reason = match state.auxiliary >> 16 {
                1 => FaultReason::Unmapped,
                2 => FaultReason::Permission,
                3 => FaultReason::AddressOverflow,
                _ => return Err(HostError::InvalidRequest),
            };
            (
                ExitReason::MemoryFault {
                    fault: MemoryFault {
                        address: GuestAddress(state.detail),
                        access,
                        reason,
                    },
                    length: width,
                },
                None,
                None,
            )
        }
        4 => (
            ExitReason::Unsupported(UnsupportedFeature::Opcode),
            None,
            None,
        ),
        5 => (ExitReason::Gate { id: state.vector }, None, None),
        6 => (ExitReason::CodeInvalidated, None, None),
        8 => (
            ExitReason::Infrastructure(InfrastructureFailure::HelperRejected),
            None,
            None,
        ),
        9 => {
            let (reason, cause, completed) = exception(state)?;
            (
                reason,
                Some(EventOrigin32::Exception(cause)),
                if completed {
                    Some(site(state, state.auxiliary, work)?)
                } else {
                    None
                },
            )
        }
        10 => {
            if state.auxiliary != 0 {
                return Err(HostError::InvalidRequest);
            }
            let kind = match state.vector {
                0..=255 => SystemEntryKind32::Interrupt(state.vector as u8),
                0x100 => SystemEntryKind32::Sysenter,
                _ => return Err(HostError::InvalidRequest),
            };
            (
                ExitReason::SystemEntry { kind },
                Some(EventOrigin32::SystemEntry(kind)),
                Some(site(state, state.detail, work)?),
            )
        }
        _ => return Err(HostError::InvalidRequest),
    };
    Ok(result)
}

fn site(state: &CpuState, pc: u32, work: u32) -> Result<CpuInstructionSite32, HostError> {
    Ok(CpuInstructionSite32 {
        eip: pc,
        retired: state
            .retired
            .checked_sub(1)
            .ok_or(HostError::InvalidRequest)?,
        flags: state.eflags,
        ss_debug_shadow: false,
        work_consumed: Some(work.checked_sub(1).ok_or(HostError::InvalidRequest)?),
    })
}

fn exception(state: &CpuState) -> Result<(ExitReason, ExceptionCause32, bool), HostError> {
    let selector = || SelectorErrorCode32::from_bits(state.detail).ok_or(HostError::InvalidRequest);
    let result = match (state.vector, state.detail) {
        (0, 0) => (
            ExitReason::DivideError,
            ExceptionCause32::DivideError,
            false,
        ),
        (1, 0) => (
            ExitReason::DebugTrap,
            ExceptionCause32::Debug(DebugCause32::InstructionBreakpointFault),
            false,
        ),
        (1, 1) => (
            ExitReason::DebugTrap,
            ExceptionCause32::Debug(DebugCause32::Int1),
            true,
        ),
        (3, 0) => (
            ExitReason::Breakpoint,
            ExceptionCause32::BreakpointInt3,
            true,
        ),
        (4, 0) => (ExitReason::Overflow, ExceptionCause32::OverflowInto, true),
        (5, 0) => (ExitReason::BoundRange, ExceptionCause32::BoundRange, false),
        (6, 0) => (
            ExitReason::InvalidOpcode,
            ExceptionCause32::InvalidOpcode,
            false,
        ),
        (7, 0) => (
            ExitReason::DeviceNotAvailable,
            ExceptionCause32::DeviceNotAvailable,
            false,
        ),
        (10, _) => {
            let code = selector()?;
            (
                ExitReason::InvalidTss {
                    error_code: code.get(),
                },
                ExceptionCause32::InvalidTss(code),
                false,
            )
        }
        (11, _) => {
            let code = selector()?;
            (
                ExitReason::SegmentNotPresent {
                    error_code: code.get(),
                },
                ExceptionCause32::SegmentNotPresent(code),
                false,
            )
        }
        (12, _) => {
            let code = selector()?;
            (
                ExitReason::StackSegment {
                    error_code: code.get(),
                },
                ExceptionCause32::StackSegment(code),
                false,
            )
        }
        (13, _) => {
            let code = selector()?;
            (
                ExitReason::GeneralProtection {
                    error_code: code.get(),
                },
                ExceptionCause32::GeneralProtection(code),
                false,
            )
        }
        (16, 0..=0xffff) => (
            ExitReason::FloatingPoint {
                status: state.detail as u16,
            },
            ExceptionCause32::FloatingPoint {
                status: state.detail as u16,
            },
            false,
        ),
        (19, 0..=0xffff) => (
            ExitReason::SimdFloatingPoint {
                mxcsr: state.detail,
            },
            ExceptionCause32::SimdFloatingPoint {
                mxcsr: state.detail,
            },
            false,
        ),
        _ => return Err(HostError::InvalidRequest),
    };
    if !result.2 && state.auxiliary != 0 {
        return Err(HostError::InvalidRequest);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_faults_create_precise_origins_without_completed_instruction_sites() {
        for (vector, detail, reason, cause) in [
            (
                16,
                0x8081,
                ExitReason::FloatingPoint { status: 0x8081 },
                ExceptionCause32::FloatingPoint { status: 0x8081 },
            ),
            (
                19,
                0x1d84,
                ExitReason::SimdFloatingPoint { mxcsr: 0x1d84 },
                ExceptionCause32::SimdFloatingPoint { mxcsr: 0x1d84 },
            ),
        ] {
            let state = CpuState {
                status: 9,
                vector,
                detail,
                retired: 7,
                eip: 0x1234,
                ..Default::default()
            };
            assert_eq!(
                decode(&state, 8).unwrap(),
                (reason, Some(EventOrigin32::Exception(cause)), None)
            );
        }
    }

    #[test]
    fn int1_trap_and_instruction_breakpoint_fault_keep_distinct_origins_and_progress() {
        let mut state = CpuState {
            status: 9,
            vector: 1,
            detail: 1,
            auxiliary: 0,
            retired: 3,
            eip: 1,
            eflags: 0x246,
            ..Default::default()
        };
        let (reason, origin, site) = decode(&state, 5).unwrap();
        assert_eq!(reason, ExitReason::DebugTrap);
        assert_eq!(
            origin,
            Some(EventOrigin32::Exception(ExceptionCause32::Debug(
                DebugCause32::Int1
            )))
        );
        assert_eq!(
            site,
            Some(CpuInstructionSite32 {
                eip: 0,
                retired: 2,
                flags: 0x246,
                ss_debug_shadow: false,
                work_consumed: Some(4)
            })
        );
        state.detail = 0;
        let (_, origin, site) = decode(&state, 5).unwrap();
        assert_eq!(
            origin,
            Some(EventOrigin32::Exception(ExceptionCause32::Debug(
                DebugCause32::InstructionBreakpointFault
            )))
        );
        assert_eq!(site, None);
    }

    #[test]
    fn completed_interrupt_and_sysenter_require_one_real_retirement_and_work() {
        for (vector, kind) in [
            (0x2e, SystemEntryKind32::Interrupt(0x2e)),
            (0x100, SystemEntryKind32::Sysenter),
        ] {
            let mut state = CpuState {
                status: 10,
                vector,
                detail: 0x1000,
                retired: 1,
                eip: 0x1002,
                ..Default::default()
            };
            assert_eq!(
                decode(&state, 1).unwrap(),
                (
                    ExitReason::SystemEntry { kind },
                    Some(EventOrigin32::SystemEntry(kind)),
                    Some(CpuInstructionSite32 {
                        eip: 0x1000,
                        retired: 0,
                        flags: 2,
                        ss_debug_shadow: false,
                        work_consumed: Some(0)
                    })
                )
            );
            state.retired = 0;
            assert!(decode(&state, 1).is_err());
            state.retired = 1;
            assert!(decode(&state, 0).is_err());
        }
    }

    #[test]
    fn wide_memory_fault_geometry_preserves_full_width_separate_from_access() {
        let state = CpuState {
            status: 3,
            vector: 832,
            detail: 0x3000,
            auxiliary: 64 | 2 << 8 | 2 << 16,
            ..Default::default()
        };
        assert_eq!(
            decode(&state, 0).unwrap(),
            (
                ExitReason::MemoryFault {
                    fault: MemoryFault {
                        address: GuestAddress(0x3000),
                        access: Access::Write,
                        reason: FaultReason::Permission
                    },
                    length: 832
                },
                None,
                None
            )
        );
        let corrupted = CpuState {
            auxiliary: 832 | 2 << 8 | 2 << 16,
            ..state
        };
        assert!(decode(&corrupted, 0).is_err());
    }

    #[test]
    fn unknown_vectors_status_and_invalid_numeric_or_selector_payloads_are_rejected() {
        for state in [
            CpuState {
                status: 7,
                ..Default::default()
            },
            CpuState {
                status: 9,
                vector: 2,
                ..Default::default()
            },
            CpuState {
                status: 9,
                vector: 16,
                detail: 0x10000,
                ..Default::default()
            },
            CpuState {
                status: 9,
                vector: 13,
                detail: 0x10000,
                ..Default::default()
            },
            CpuState {
                status: 10,
                vector: 0x101,
                retired: 1,
                ..Default::default()
            },
        ] {
            assert!(decode(&state, 1).is_err());
        }
    }
}
