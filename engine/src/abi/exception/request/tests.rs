use super::*;

fn request(tag: u32, detail: u32) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..16].copy_from_slice(&[82, 51, 69, 86, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (offset, value) in [
        (16, tag),
        (20, detail),
        (32, 0x12345678_u32),
        (36, 0xfffffff0),
        (40, 0x10002),
        (44, 0x1b),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn literal_event_origins_preserve_software_debug_fault_and_partial_rep_distinctions() {
    for (tag, detail, expected) in [
        (0, 1, EventOrigin32::SoftwareInt { vector: 1 }),
        (0, 3, EventOrigin32::SoftwareInt { vector: 3 }),
        (
            1,
            0,
            EventOrigin32::Exception(ExceptionCause32::DivideError),
        ),
        (2, 0, EventOrigin32::Exception(ExceptionCause32::BoundRange)),
        (
            3,
            0,
            EventOrigin32::Exception(ExceptionCause32::InvalidOpcode),
        ),
        (
            4,
            0x8081,
            EventOrigin32::Exception(ExceptionCause32::FloatingPoint { status: 0x8081 }),
        ),
        (
            5,
            0x1f81,
            EventOrigin32::Exception(ExceptionCause32::SimdFloatingPoint { mxcsr: 0x1f81 }),
        ),
        (
            6,
            0x4b,
            EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
                SelectorErrorCode32::from_bits(0x4b).unwrap(),
            )),
        ),
        (
            7,
            0x48,
            EventOrigin32::Exception(ExceptionCause32::SegmentNotPresent(
                SelectorErrorCode32::from_bits(0x48).unwrap(),
            )),
        ),
        (
            8,
            0,
            EventOrigin32::Exception(ExceptionCause32::StackSegment(SelectorErrorCode32::null(
                false,
            ))),
        ),
        (
            9,
            0,
            EventOrigin32::Exception(ExceptionCause32::BreakpointInt3),
        ),
        (
            10,
            0,
            EventOrigin32::Exception(ExceptionCause32::OverflowInto),
        ),
        (
            11,
            0,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::Int1)),
        ),
        (
            11,
            1,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::SingleStep(
                TrapPoint32::CompletedInstruction,
            ))),
        ),
        (
            11,
            2,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::SingleStep(
                TrapPoint32::RepetitionInProgress,
            ))),
        ),
        (
            11,
            3,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::DataBreakpoint(
                TrapPoint32::CompletedInstruction,
            ))),
        ),
        (
            11,
            4,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::DataBreakpoint(
                TrapPoint32::RepetitionInProgress,
            ))),
        ),
        (
            11,
            5,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::IoBreakpoint(
                TrapPoint32::CompletedInstruction,
            ))),
        ),
        (
            11,
            6,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::IoBreakpoint(
                TrapPoint32::RepetitionInProgress,
            ))),
        ),
        (
            11,
            7,
            EventOrigin32::Exception(ExceptionCause32::Debug(
                DebugCause32::InstructionBreakpointFault,
            )),
        ),
        (
            11,
            8,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::GeneralDetectFault)),
        ),
        (
            11,
            9,
            EventOrigin32::Exception(ExceptionCause32::Debug(DebugCause32::TaskSwitchTrap)),
        ),
    ] {
        assert_eq!(
            decode_event_request(&request(tag, detail)),
            Ok(EventRequest32 {
                origin: expected,
                input: FrameInput32 {
                    eip: 0x12345678,
                    esp: 0xfffffff0,
                    flags: 0x10002,
                    cs: 0x1b
                },
            })
        );
    }
}

#[test]
fn malformed_or_uncertified_requests_never_acquire_an_event_origin() {
    for (tag, detail) in [
        (0, 256),
        (1, 1),
        (2, 1),
        (3, 1),
        (4, 0x10000),
        (5, 0x10000),
        (6, 0x10000),
        (7, 0x10000),
        (8, 0x10000),
        (9, 1),
        (10, 1),
        (11, 10),
        (12, 0),
        (u32::MAX, 0),
    ] {
        assert!(
            decode_event_request(&request(tag, detail)).is_err(),
            "tag={tag} detail={detail}"
        );
    }
    for offset in [0, 4, 6, 8, 12, 24, 28, 48, 52, 56, 60] {
        let mut bytes = request(3, 0);
        bytes[offset] ^= 1;
        assert!(decode_event_request(&bytes).is_err(), "offset={offset}");
    }
    for (offset, value) in [(40, 0_u32), (40, 0x20002), (44, 0x10000)] {
        let mut bytes = request(3, 0);
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(decode_event_request(&bytes).is_err());
    }
    assert_eq!(
        decode_event_request(&request(3, 0)[..63]),
        Err(AbiError::Length)
    );
}
