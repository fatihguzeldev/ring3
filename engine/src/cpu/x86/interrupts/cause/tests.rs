use super::*;

#[test]
fn application_origins_keep_vector_and_exact_error_code_presence_distinct() {
    for (origin, vector, code) in [
        (
            EventOrigin32::SystemEntry(SystemEntryKind32::Interrupt(0x2e)),
            0x2e,
            None,
        ),
        (
            EventOrigin32::SystemEntry(SystemEntryKind32::Interrupt(0x29)),
            0x29,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::BreakpointInt3),
            3,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::OverflowInto),
            4,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::Int1)),
            1,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::BoundRange),
            5,
            None,
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::InvalidTss(
                SelectorErrorCode32::from_bits(0).unwrap(),
            )),
            10,
            Some(0),
        ),
        (
            EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
                SelectorErrorCode32::from_bits(0x402).unwrap(),
            )),
            13,
            Some(0x402),
        ),
    ] {
        assert_eq!((origin.vector(), origin.error_code()), (Some(vector), code));
    }
    for bits in [0, 3, 0x4d, 0x402, 0xffff] {
        assert_eq!(SelectorErrorCode32::from_bits(bits).unwrap().get(), bits);
    }
    assert!(SelectorErrorCode32::from_bits(0x10000).is_none());
}

#[test]
fn exception_class_preserves_fault_and_trap_distinctions() {
    for cause in [
        ExceptionCause32::BoundRange,
        ExceptionCause32::DivideError,
        ExceptionCause32::InvalidOpcode,
        ExceptionCause32::Debug(DebugCause32::InstructionBreakpointFault),
        ExceptionCause32::Debug(DebugCause32::GeneralDetectFault),
    ] {
        assert_eq!(cause.class(), ExceptionClass32::Fault);
    }
    for cause in [
        ExceptionCause32::BreakpointInt3,
        ExceptionCause32::OverflowInto,
        ExceptionCause32::Debug(DebugCause32::Int1),
        ExceptionCause32::Debug(DebugCause32::SingleStep(TrapPoint32::CompletedInstruction)),
        ExceptionCause32::Debug(DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress)),
    ] {
        assert_eq!(cause.class(), ExceptionClass32::Trap);
    }
}

#[test]
fn sysenter_origin_has_no_invented_interrupt_vector_or_error_code() {
    let origin = EventOrigin32::SystemEntry(SystemEntryKind32::Sysenter);
    assert_eq!(origin.vector(), None);
    assert_eq!(origin.error_code(), None);
}
