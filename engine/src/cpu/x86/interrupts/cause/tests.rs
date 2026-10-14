use super::*;

#[test]
fn origins_keep_vector_gate_privilege_error_presence_and_original_codes_distinct() {
    for (origin, vector, external, code) in [
        (EventOrigin32::SoftwareInt { vector: 13 }, 13, false, None),
        (
            EventOrigin32::Exception(ExceptionCause32::BreakpointInt3),
            3,
            false,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::OverflowInto),
            4,
            false,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::Int1)),
            1,
            true,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::BoundRange),
            5,
            true,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
                SelectorErrorCode32::null(false),
            )),
            13,
            true,
            Some(0),
        ),
    ] {
        assert_eq!(
            (
                origin.vector(),
                origin.external(),
                origin.checks_gate_dpl(),
                origin.error_code()
            ),
            (vector, external, !external, code)
        );
    }
    assert_eq!(SelectorErrorCode32::idt(0, true).get(), 3);
    assert_eq!(SelectorErrorCode32::idt(0x80, false).get(), 0x402);
    assert_eq!(SelectorErrorCode32::descriptor(0x4f, true).get(), 0x4d);
    assert!(SelectorErrorCode32::from_bits(0x10000).is_none());
}

#[test]
fn saved_rf_depends_on_origin_and_repeat_progress_not_fault_trap_shorthand() {
    let origins = [
        (
            EventOrigin32::Exception(ExceptionCause32::DivideError),
            [0x10002, 0x10002],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::GeneralDetectFault)),
            [0x10002, 0x10002],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(
                DebugCause32::InstructionBreakpointFault,
            )),
            [2, 0x10002],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::SingleStep(
                TrapPoint32::CompletedInstruction,
            ))),
            [2, 0x10002],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::SingleStep(
                TrapPoint32::RepetitionInProgress,
            ))),
            [0x10002, 0x10002],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::Int1)),
            [2, 2],
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::BreakpointInt3),
            [2, 2],
        ),
        (EventOrigin32::SoftwareInt { vector: 0x80 }, [2, 2]),
    ];
    for (origin, literal) in origins {
        assert_eq!(
            [origin.saved_flags(2), origin.saved_flags(0x10002)],
            literal
        );
    }
    assert_eq!(
        ExceptionCause32::BoundRange.class(),
        ExceptionClass32::Fault
    );
    assert_eq!(
        ExceptionCause32::BoundRange.escalation_class(),
        EscalationClass32::Benign
    );
    assert_eq!(
        ExceptionCause32::DivideError.escalation_class(),
        EscalationClass32::Contributory
    );
}
