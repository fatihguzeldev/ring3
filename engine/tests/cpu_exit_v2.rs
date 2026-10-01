use ring3_engine::abi::AbiError;
use ring3_engine::abi::memory_helper::encode_helper_result;
use ring3_engine::abi::x86::{
    EXIT_VERSION_2, decode_exit, encode_exit, encode_exit_v2, encode_state,
};
use ring3_engine::cpu::x86::State32;
use ring3_engine::cpu::{ExecutionExit, ExitReason, InfrastructureFailure, UnsupportedFeature};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryFault};

const INFRASTRUCTURE_GOLDEN: [u8; 40] = [
    0x52, 0x33, 0x45, 0x58, 0x02, 0x00, 0x01, 0x00, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x07, 0x00, 0x00, 0x00, 0x44, 0x33, 0x22, 0x11, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn set_word(record: &mut [u8; 40], offset: usize, value: u32) {
    record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn record(version: u8, fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[
        0x52, 0x33, 0x45, 0x58, version, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0,
    ]);
    for (index, value) in fields.into_iter().enumerate() {
        set_word(&mut bytes, 16 + index * 4, value);
    }
    bytes
}

#[test]
fn infrastructure_matches_an_independent_version_two_literal() {
    assert_eq!(EXIT_VERSION_2, 2);
    let exit = ExecutionExit {
        retired: 0x1122_3344,
        reason: ExitReason::Infrastructure(InfrastructureFailure::VersionExhausted),
    };
    let mut output = [0xa5; 40];
    encode_exit_v2(&exit, &mut output).unwrap();
    assert_eq!(output, INFRASTRUCTURE_GOLDEN);
    assert_eq!(decode_exit(&INFRASTRUCTURE_GOLDEN), Ok(exit));
}

#[test]
fn infrastructure_details_are_explicit_and_fault_fields_are_zero() {
    for (failure, detail) in [
        (InfrastructureFailure::VersionExhausted, 1),
        (InfrastructureFailure::HelperProtocol, 2),
        (InfrastructureFailure::HelperRejected, 3),
    ] {
        let exit = ExecutionExit {
            retired: u32::MAX,
            reason: ExitReason::Infrastructure(failure),
        };
        let expected = record(2, [7, u32::MAX, detail, 0, 0, 0]);
        let mut output = [0xa5; 40];
        encode_exit_v2(&exit, &mut output).unwrap();
        assert_eq!(output, expected);
        assert_eq!(decode_exit(&expected), Ok(exit));
    }
}

#[test]
fn all_legacy_reasons_have_the_same_fields_in_both_versions() {
    let fault = MemoryFault {
        address: GuestAddress(0x1234_5678),
        access: Access::Write,
        reason: FaultReason::Permission,
    };
    for (reason, fields) in [
        (ExitReason::Budget, [1, 17, 0, 0, 0, 0]),
        (ExitReason::Cancelled, [2, 17, 0, 0, 0, 0]),
        (ExitReason::NeedCode, [3, 17, 0, 0, 0, 0]),
        (
            ExitReason::Unsupported(UnsupportedFeature::Opcode),
            [4, 17, 1, 0, 0, 0],
        ),
        (
            ExitReason::Unsupported(UnsupportedFeature::FloatingPoint),
            [4, 17, 2, 0, 0, 0],
        ),
        (
            ExitReason::Unsupported(UnsupportedFeature::Simd),
            [4, 17, 3, 0, 0, 0],
        ),
        (
            ExitReason::Unsupported(UnsupportedFeature::Segment),
            [4, 17, 4, 0, 0, 0],
        ),
        (
            ExitReason::Unsupported(UnsupportedFeature::RepeatedString),
            [4, 17, 5, 0, 0, 0],
        ),
        (
            ExitReason::Unsupported(UnsupportedFeature::Privileged),
            [4, 17, 6, 0, 0, 0],
        ),
        (
            ExitReason::MemoryFault { fault, length: 4 },
            [5, 17, 2, 0x1234_5678, 2, 4],
        ),
        (ExitReason::CodeInvalidated, [6, 17, 0, 0, 0, 0]),
    ] {
        let exit = ExecutionExit {
            retired: 17,
            reason,
        };
        for version in [1, 2] {
            let expected = record(version, fields);
            let mut output = [0xa5; 40];
            if version == 1 {
                encode_exit(&exit, &mut output).unwrap();
            } else {
                encode_exit_v2(&exit, &mut output).unwrap();
            }
            assert_eq!(output, expected);
            assert_eq!(decode_exit(&expected), Ok(exit));
        }
    }
}

#[test]
fn version_one_keeps_its_closed_reason_set_and_atomic_encoder() {
    for failure in [
        InfrastructureFailure::VersionExhausted,
        InfrastructureFailure::HelperProtocol,
        InfrastructureFailure::HelperRejected,
    ] {
        let exit = ExecutionExit {
            retired: 9,
            reason: ExitReason::Infrastructure(failure),
        };
        let mut output = [0xa5; 40];
        assert_eq!(encode_exit(&exit, &mut output), Err(AbiError::Exit));
        assert_eq!(output, [0xa5; 40]);
    }
    for detail in [0, 1, 2, 3, 4, u32::MAX] {
        assert_eq!(
            decode_exit(&record(1, [7, 0, detail, 0, 0, 0])),
            Err(AbiError::Exit)
        );
    }
}

#[test]
fn version_two_rejects_unknown_tags_details_and_infrastructure_fault_fields() {
    for reason in [0, 8, u32::MAX] {
        assert_eq!(
            decode_exit(&record(2, [reason, 0, 0, 0, 0, 0])),
            Err(AbiError::Exit)
        );
    }
    for detail in [0, 4, u32::MAX] {
        assert_eq!(
            decode_exit(&record(2, [7, 0, detail, 0, 0, 0])),
            Err(AbiError::Exit)
        );
    }
    for offset in [28, 32, 36] {
        let mut bytes = INFRASTRUCTURE_GOLDEN;
        set_word(&mut bytes, offset, 1);
        assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
    }
}

#[test]
fn malformed_version_two_headers_remain_checked() {
    for offset in 0..16 {
        let mut bytes = INFRASTRUCTURE_GOLDEN;
        bytes[offset] ^= 0x80;
        let error = match offset {
            0..=3 => AbiError::Magic,
            4..=5 => AbiError::Version,
            6..=7 => AbiError::Profile,
            8..=11 => AbiError::Length,
            12..=15 => AbiError::Reserved,
            _ => unreachable!(),
        };
        assert_eq!(decode_exit(&bytes), Err(error));
    }
    for version in [0, 3, 255] {
        assert_eq!(
            decode_exit(&record(version, [1, 0, 0, 0, 0, 0])),
            Err(AbiError::Version)
        );
    }
}

#[test]
fn all_truncations_and_oversized_buffers_fail_before_output_changes() {
    let exit = ExecutionExit {
        retired: 0,
        reason: ExitReason::Infrastructure(InfrastructureFailure::HelperProtocol),
    };
    for length in (0..40).chain([41, 80]) {
        let mut output = vec![0xa5; length];
        assert_eq!(encode_exit_v2(&exit, &mut output), Err(AbiError::Length));
        assert_eq!(encode_exit(&exit, &mut output), Err(AbiError::Length));
        assert_eq!(output, vec![0xa5; length]);
        let mut input = INFRASTRUCTURE_GOLDEN.to_vec();
        input.resize(length, 0);
        assert_eq!(decode_exit(&input), Err(AbiError::Length));
    }
}

#[test]
fn version_two_fault_validation_remains_atomic() {
    for (address, access, reason, length) in [
        (0x1000, Access::Read, FaultReason::Unmapped, 3),
        (0x1000, Access::Execute, FaultReason::Permission, 16),
        (0xffff_fffd, Access::Write, FaultReason::Permission, 4),
        (0xffff_fffc, Access::Write, FaultReason::AddressOverflow, 4),
    ] {
        let exit = ExecutionExit {
            retired: 0,
            reason: ExitReason::MemoryFault {
                fault: MemoryFault {
                    address: GuestAddress(address),
                    access,
                    reason,
                },
                length,
            },
        };
        let mut output = [0xa5; 40];
        assert_eq!(encode_exit_v2(&exit, &mut output), Err(AbiError::Exit));
        assert_eq!(output, [0xa5; 40]);
    }
}

#[test]
fn exit_version_two_does_not_change_state_or_helper_wire_versions() {
    let mut state = [0xa5; 56];
    encode_state(&State32::default(), &mut state).unwrap();
    let mut helper = [0xa5; 40];
    encode_helper_result(Ok(0), &mut helper).unwrap();
    assert_eq!(&state[4..8], &[1, 0, 1, 0]);
    assert_eq!(&helper[4..8], &[1, 0, 1, 0]);
}
