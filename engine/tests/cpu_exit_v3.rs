use ring3_engine::abi::AbiError;
use ring3_engine::abi::memory_helper::encode_helper_result;
use ring3_engine::abi::x86::{
    EXIT_VERSION_3, decode_exit, decode_state, encode_exit, encode_exit_v2, encode_exit_v3,
    encode_state,
};
use ring3_engine::cpu::x86::State32;
use ring3_engine::cpu::{ExecutionExit, ExitReason, InfrastructureFailure, UnsupportedFeature};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryFault};

const GATE_GOLDEN: [u8; 40] = [
    0x52, 0x33, 0x45, 0x58, 0x03, 0x00, 0x01, 0x00, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00, 0x00, 0x00, 0x00,
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
fn numeric_gate_matches_an_independent_version_three_literal() {
    assert_eq!(EXIT_VERSION_3, 3);
    let expected = ExecutionExit {
        retired: 0x5566_7788,
        reason: ExitReason::Gate { id: 0x1122_3344 },
    };
    let mut output = [0xa5; 40];
    encode_exit_v3(&expected, &mut output).unwrap();
    assert_eq!(output, GATE_GOLDEN);
    assert_eq!(decode_exit(&GATE_GOLDEN), Ok(expected));
}

#[test]
fn gate_ids_cover_the_nonzero_u32_range_and_keep_fault_fields_zero() {
    for id in [1, 0x8000_0000, u32::MAX] {
        let expected = ExecutionExit {
            retired: u32::MAX,
            reason: ExitReason::Gate { id },
        };
        let bytes = record(3, [8, u32::MAX, id, 0, 0, 0]);
        let mut output = [0xa5; 40];
        encode_exit_v3(&expected, &mut output).unwrap();
        assert_eq!(output, bytes);
        assert_eq!(decode_exit(&bytes), Ok(expected));
    }
}

#[test]
fn old_encoders_and_wire_versions_atomically_reject_gate_exits() {
    for id in [1, 0x1122_3344, u32::MAX] {
        let gate = ExecutionExit {
            retired: 9,
            reason: ExitReason::Gate { id },
        };
        for encode in [encode_exit, encode_exit_v2] {
            let mut output = [0xa5; 40];
            assert_eq!(encode(&gate, &mut output), Err(AbiError::Exit));
            assert_eq!(output, [0xa5; 40]);
        }
        for version in [1, 2] {
            assert_eq!(
                decode_exit(&record(version, [8, 9, id, 0, 0, 0])),
                Err(AbiError::Exit)
            );
        }
    }
}

#[test]
fn zero_gate_id_and_nonzero_gate_fault_fields_are_rejected() {
    let invalid = ExecutionExit {
        retired: 0,
        reason: ExitReason::Gate { id: 0 },
    };
    let mut output = [0xa5; 40];
    assert_eq!(encode_exit_v3(&invalid, &mut output), Err(AbiError::Exit));
    assert_eq!(output, [0xa5; 40]);
    assert_eq!(
        decode_exit(&record(3, [8, 0, 0, 0, 0, 0])),
        Err(AbiError::Exit)
    );
    for offset in [28, 32, 36] {
        let mut malformed = GATE_GOLDEN;
        set_word(&mut malformed, offset, 1);
        assert_eq!(decode_exit(&malformed), Err(AbiError::Exit));
    }
}

#[test]
fn all_closed_existing_reasons_keep_their_exact_fields_in_version_three() {
    let reasons = [
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
        (ExitReason::CodeInvalidated, [6, 17, 0, 0, 0, 0]),
        (
            ExitReason::Infrastructure(InfrastructureFailure::VersionExhausted),
            [7, 17, 1, 0, 0, 0],
        ),
        (
            ExitReason::Infrastructure(InfrastructureFailure::HelperProtocol),
            [7, 17, 2, 0, 0, 0],
        ),
        (
            ExitReason::Infrastructure(InfrastructureFailure::HelperRejected),
            [7, 17, 3, 0, 0, 0],
        ),
    ];
    for (reason, fields) in reasons {
        let exit = ExecutionExit {
            retired: 17,
            reason,
        };
        let expected = record(3, fields);
        let mut output = [0xa5; 40];
        encode_exit_v3(&exit, &mut output).unwrap();
        assert_eq!(output, expected);
        assert_eq!(decode_exit(&expected), Ok(exit));
        let mut old = [0xa5; 40];
        encode_exit_v2(&exit, &mut old).unwrap();
        assert_eq!(&old[16..], &expected[16..]);
    }
}

#[test]
fn version_three_retains_read_write_execute_and_overflow_fault_fields() {
    for (address, access, access_tag, reason, detail, length) in [
        (0x1234_5678, Access::Read, 1, FaultReason::Unmapped, 1, 1),
        (0x1234_5678, Access::Write, 2, FaultReason::Permission, 2, 4),
        (0xffff_fffc, Access::Write, 2, FaultReason::Permission, 2, 4),
        (
            0x1234_5678,
            Access::Execute,
            3,
            FaultReason::Permission,
            2,
            15,
        ),
        (
            u32::MAX,
            Access::Execute,
            3,
            FaultReason::AddressOverflow,
            3,
            2,
        ),
    ] {
        let exit = ExecutionExit {
            retired: 17,
            reason: ExitReason::MemoryFault {
                fault: MemoryFault {
                    address: GuestAddress(address),
                    access,
                    reason,
                },
                length,
            },
        };
        let expected = record(3, [5, 17, detail, address, access_tag, length]);
        let mut output = [0xa5; 40];
        encode_exit_v3(&exit, &mut output).unwrap();
        assert_eq!(output, expected);
        assert_eq!(decode_exit(&expected), Ok(exit));
    }
}

#[test]
fn malformed_version_three_headers_keep_precise_field_errors() {
    for offset in 0..16 {
        let mut malformed = GATE_GOLDEN;
        malformed[offset] ^= 0x80;
        let expected = match offset {
            0..=3 => AbiError::Magic,
            4..=5 => AbiError::Version,
            6..=7 => AbiError::Profile,
            8..=11 => AbiError::Length,
            12..=15 => AbiError::Reserved,
            _ => unreachable!(),
        };
        assert_eq!(decode_exit(&malformed), Err(expected));
    }
    for version in [0, 4, 255] {
        assert_eq!(
            decode_exit(&record(version, [8, 0, 1, 0, 0, 0])),
            Err(AbiError::Version)
        );
    }
}

#[test]
fn truncations_oversized_records_and_invalid_values_do_not_change_output() {
    for id in [0, 17] {
        let exit = ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        };
        for length in (0..40).chain([41, 80]) {
            let mut output = vec![0xa5; length];
            assert_eq!(encode_exit_v3(&exit, &mut output), Err(AbiError::Length));
            assert_eq!(output, vec![0xa5; length]);
        }
    }
    for length in (0..40).chain([41, 80]) {
        let mut input = GATE_GOLDEN.to_vec();
        input.resize(length, 0);
        assert_eq!(decode_exit(&input), Err(AbiError::Length));
    }
}

#[test]
fn version_three_does_not_relax_closed_tags_details_or_reserved_fields() {
    for fields in [
        [0, 0, 0, 0, 0, 0],
        [9, 0, 0, 0, 0, 0],
        [u32::MAX, 0, 0, 0, 0, 0],
        [1, 0, 1, 0, 0, 0],
        [2, 0, 0, 1, 0, 0],
        [3, 0, 0, 0, 1, 0],
        [6, 0, 0, 0, 0, 1],
        [4, 0, 0, 0, 0, 0],
        [4, 0, 7, 0, 0, 0],
        [4, 0, 1, 1, 0, 0],
        [7, 0, 0, 0, 0, 0],
        [7, 0, 4, 0, 0, 0],
        [7, 0, 1, 0, 1, 0],
        [5, 0, 0, 0x1000, 1, 4],
        [5, 0, 4, 0x1000, 1, 4],
        [5, 0, 1, 0x1000, 0, 4],
        [5, 0, 1, 0x1000, 4, 4],
    ] {
        assert_eq!(decode_exit(&record(3, fields)), Err(AbiError::Exit));
    }
    assert_eq!(
        decode_exit(&record(1, [7, 0, 1, 0, 0, 0])),
        Err(AbiError::Exit)
    );
}

#[test]
fn invalid_fault_widths_and_overflow_relationships_keep_atomic_validation() {
    for (address, access, access_tag, reason, detail, length) in [
        (0x1000, Access::Read, 1, FaultReason::Unmapped, 1, 3),
        (0x1000, Access::Write, 2, FaultReason::Permission, 2, 0),
        (0x1000, Access::Execute, 3, FaultReason::Permission, 2, 16),
        (0xffff_fffd, Access::Write, 2, FaultReason::Permission, 2, 4),
        (
            0xffff_fffc,
            Access::Write,
            2,
            FaultReason::AddressOverflow,
            3,
            4,
        ),
        (
            u32::MAX,
            Access::Read,
            1,
            FaultReason::AddressOverflow,
            3,
            1,
        ),
    ] {
        let invalid = ExecutionExit {
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
        assert_eq!(encode_exit_v3(&invalid, &mut output), Err(AbiError::Exit));
        assert_eq!(output, [0xa5; 40]);
        assert_eq!(
            decode_exit(&record(3, [5, 0, detail, address, access_tag, length])),
            Err(AbiError::Exit)
        );
    }
}

#[test]
fn exit_version_three_leaves_state_and_helper_headers_at_version_one() {
    let mut state = [0xa5; 56];
    encode_state(&State32::default(), &mut state).unwrap();
    let mut helper = [0xa5; 40];
    encode_helper_result(Ok(0), &mut helper).unwrap();
    assert_eq!(&state[4..8], &[1, 0, 1, 0]);
    assert_eq!(&helper[4..8], &[1, 0, 1, 0]);
    state[4] = 3;
    assert_eq!(decode_state(&state), Err(AbiError::Version));
}
