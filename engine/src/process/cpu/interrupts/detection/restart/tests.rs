use super::*;
use crate::{
    cpu::x86::TrapPoint32,
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
};

fn input() -> FrameInput32 {
    FrameInput32 {
        eip: 0x1003,
        esp: 0x7800,
        flags: 0x240fd7,
        cs: 0x1b,
    }
}
fn restart() -> InstructionTrapRestart32 {
    InstructionTrapRestart32 {
        eip: 0x1000,
        retired: 7,
        flags: 0x250fd7,
        ss_debug_shadow: true,
        work_consumed: Some(11),
    }
}
fn event(cause: ExceptionCause32) -> DetectedEvent32 {
    DetectedEvent32::Exception(EventOrigin32::Exception(cause))
}

#[test]
fn cc_into_and_f1_require_trusted_restart_while_completed_tf_and_partial_rep_forbid_it() {
    for cause in [
        ExceptionCause32::BreakpointInt3,
        ExceptionCause32::OverflowInto,
        ExceptionCause32::Debug(DebugCause32::Int1),
    ] {
        assert!(valid_provenance(
            event(cause),
            Some(restart()),
            input(),
            8,
            false,
            12
        ));
        assert!(!valid_provenance(event(cause), None, input(), 8, false, 12));
    }
    for detection in [
        event(ExceptionCause32::DivideError),
        event(ExceptionCause32::Debug(DebugCause32::SingleStep(
            TrapPoint32::CompletedInstruction,
        ))),
        event(ExceptionCause32::Debug(DebugCause32::SingleStep(
            TrapPoint32::RepetitionInProgress,
        ))),
        DetectedEvent32::NeedsPagingAuthority {
            fault: MemoryFault {
                address: GuestAddress(3),
                access: Access::Read,
                reason: FaultReason::Unmapped,
            },
            length: 4,
        },
    ] {
        assert!(valid_provenance(detection, None, input(), 8, false, 12));
        assert!(!valid_provenance(
            detection,
            Some(restart()),
            input(),
            8,
            false,
            12
        ));
    }
    // success frame remains at the detector's continuation; only failed instruction delivery restarts.
    assert_eq!(
        restart().attempted_input(input()),
        FrameInput32 {
            eip: 0x1000,
            ..input()
        }
    );
    assert_eq!(
        restart().unattempted_input(input()),
        FrameInput32 {
            eip: 0x1000,
            flags: 0x250fd7,
            ..input()
        }
    );
}

#[test]
fn restart_retirement_flags_shadow_and_optional_work_are_checked_without_opcode_length_inference() {
    let detection = event(ExceptionCause32::Debug(DebugCause32::Int1));
    for (restart, input, retired, shadow, work) in [
        (
            InstructionTrapRestart32 {
                retired: 8,
                ..restart()
            },
            input(),
            8,
            false,
            12,
        ),
        (
            InstructionTrapRestart32 {
                retired: u32::MAX,
                ..restart()
            },
            input(),
            0,
            false,
            12,
        ),
        (
            InstructionTrapRestart32 {
                flags: 0,
                ..restart()
            },
            input(),
            8,
            false,
            12,
        ),
        (
            InstructionTrapRestart32 {
                flags: 0x20002,
                ..restart()
            },
            input(),
            8,
            false,
            12,
        ),
        (
            restart(),
            FrameInput32 {
                flags: 0x250fd7,
                ..input()
            },
            8,
            false,
            12,
        ),
        (restart(), input(), 8, true, 12),
        (restart(), input(), 8, false, 11),
        (
            InstructionTrapRestart32 {
                work_consumed: Some(u32::MAX),
                ..restart()
            },
            input(),
            8,
            false,
            0,
        ),
    ] {
        assert!(!valid_provenance(
            detection,
            Some(restart),
            input,
            retired,
            shadow,
            work
        ));
    }
    assert!(
        valid_provenance(
            detection,
            Some(InstructionTrapRestart32 {
                work_consumed: None,
                ..restart()
            }),
            input(),
            8,
            false,
            99
        ),
        "non-REP modules do not charge a stale shared work record"
    );
    // only the trusted detector supplies original EIP; no scan or next-PC subtraction appears here.
    assert!(valid_provenance(
        detection,
        Some(InstructionTrapRestart32 {
            eip: 0x0fff,
            ..restart()
        }),
        input(),
        8,
        false,
        12
    ));
}
