use ring3_engine::abi::AbiError;
use ring3_engine::abi::x86::{
    ACCESS_LENGTH_OFFSET, ACCESS_OFFSET, DETAIL_OFFSET, EXIT_SIZE, FAULT_ADDRESS_OFFSET,
    REASON_OFFSET, RETIRED_OFFSET, decode_exit, encode_exit,
};
use ring3_engine::cpu::{ExecutionExit, ExitReason, UnsupportedFeature};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryFault};

const GOLDEN: [u8; 40] = [
    0x52, 0x33, 0x45, 0x58, 0x01, 0x00, 0x01, 0x00, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x44, 0x33, 0x22, 0x11, 0x02, 0x00, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12,
    0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
];

fn golden_exit() -> ExecutionExit {
    ExecutionExit {
        retired: 0x1122_3344,
        reason: ExitReason::MemoryFault {
            fault: MemoryFault {
                address: GuestAddress(0x1234_5678),
                access: Access::Read,
                reason: FaultReason::Permission,
            },
            length: 4,
        },
    }
}

fn word(record: &[u8; 40], offset: usize) -> u32 {
    u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap())
}

fn set_word(record: &mut [u8; 40], offset: usize, value: u32) {
    record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn assert_roundtrip(original: &ExecutionExit) -> [u8; 40] {
    let mut record = [0xa5; 40];
    encode_exit(original, &mut record).unwrap();
    let decoded = decode_exit(&record).unwrap();
    assert_eq!(decoded.retired, original.retired);
    assert_eq!(decoded.reason, original.reason);
    record
}

#[test]
fn public_exit_offsets_match_the_byte_contract() {
    assert_eq!(EXIT_SIZE, 40);
    assert_eq!(REASON_OFFSET, 16);
    assert_eq!(RETIRED_OFFSET, 20);
    assert_eq!(DETAIL_OFFSET, 24);
    assert_eq!(FAULT_ADDRESS_OFFSET, 28);
    assert_eq!(ACCESS_OFFSET, 32);
    assert_eq!(ACCESS_LENGTH_OFFSET, 36);
}

#[test]
fn permission_read_fault_matches_independent_literal_record() {
    let original = golden_exit();
    assert_eq!(assert_roundtrip(&original), GOLDEN);
    let decoded = decode_exit(&GOLDEN).unwrap();
    assert_eq!(decoded.retired, 0x1122_3344);
    assert_eq!(decoded.reason, original.reason);
}

#[test]
fn simple_exit_reasons_have_explicit_tags_and_zero_detail_fields() {
    for (reason, tag) in [
        (ExitReason::Budget, 1),
        (ExitReason::Cancelled, 2),
        (ExitReason::NeedCode, 3),
        (ExitReason::CodeInvalidated, 6),
    ] {
        let record = assert_roundtrip(&ExecutionExit {
            retired: u32::MAX,
            reason,
        });
        assert_eq!(word(&record, 16), tag);
        assert_eq!(word(&record, 20), u32::MAX);
        assert_eq!(&record[24..40], &[0; 16]);
    }
}

#[test]
fn unsupported_features_have_explicit_detail_tags_and_no_fault_fields() {
    for (feature, detail) in [
        (UnsupportedFeature::Opcode, 1),
        (UnsupportedFeature::FloatingPoint, 2),
        (UnsupportedFeature::Simd, 3),
        (UnsupportedFeature::Segment, 4),
        (UnsupportedFeature::RepeatedString, 5),
        (UnsupportedFeature::Privileged, 6),
    ] {
        let record = assert_roundtrip(&ExecutionExit {
            retired: 0,
            reason: ExitReason::Unsupported(feature),
        });
        assert_eq!(word(&record, 16), 4);
        assert_eq!(word(&record, 24), detail);
        assert_eq!(&record[28..40], &[0; 12]);
    }
}

#[test]
fn supported_fault_reasons_access_types_and_widths_roundtrip() {
    for (reason, detail) in [
        (FaultReason::Unmapped, 1),
        (FaultReason::Permission, 2),
        (FaultReason::AddressOverflow, 3),
    ] {
        for (access, tag) in [(Access::Read, 1), (Access::Write, 2), (Access::Execute, 3)] {
            for length in 1..=15 {
                if access != Access::Execute && ![1, 2, 4].contains(&length) {
                    continue;
                }
                if reason == FaultReason::AddressOverflow && length == 1 {
                    continue;
                }
                let address = if reason == FaultReason::AddressOverflow {
                    u32::MAX
                } else {
                    0x1234_5678
                };
                let original = ExecutionExit {
                    retired: 0x1122_3344,
                    reason: ExitReason::MemoryFault {
                        fault: MemoryFault {
                            address: GuestAddress(address),
                            access,
                            reason,
                        },
                        length,
                    },
                };
                let record = assert_roundtrip(&original);
                assert_eq!(word(&record, 16), 5);
                assert_eq!(word(&record, 24), detail);
                assert_eq!(word(&record, 28), address);
                assert_eq!(word(&record, 32), tag);
                assert_eq!(word(&record, 36), length);
            }
        }
    }
}

#[test]
fn every_truncation_and_oversized_record_is_rejected_without_output_mutation() {
    let original = golden_exit();
    for length in 0..40 {
        assert_eq!(decode_exit(&GOLDEN[..length]).err(), Some(AbiError::Length));
    }
    for length in [41, 48, 80] {
        let mut record = vec![0; length];
        record[..40].copy_from_slice(&GOLDEN);
        assert_eq!(decode_exit(&record).err(), Some(AbiError::Length));
    }
    for length in (0..40).chain([41, 48, 80]) {
        let mut output = vec![0xa5; length];
        assert_eq!(encode_exit(&original, &mut output), Err(AbiError::Length));
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
}

#[test]
fn each_malformed_header_byte_returns_its_field_error() {
    for offset in 0..16 {
        let mut record = GOLDEN;
        record[offset] ^= 0x80;
        let expected = match offset {
            0..=3 => AbiError::Magic,
            4..=5 => AbiError::Version,
            6..=7 => AbiError::Profile,
            8..=11 => AbiError::Length,
            12..=15 => AbiError::Reserved,
            _ => unreachable!(),
        };
        assert_eq!(decode_exit(&record).err(), Some(expected));
    }
}

#[test]
fn unknown_reason_detail_and_access_codes_are_rejected() {
    for code in [0, 7, u32::MAX] {
        let mut record = GOLDEN;
        set_word(&mut record, 16, code);
        assert_eq!(decode_exit(&record).err(), Some(AbiError::Exit));
    }
    for code in [0, 7, u32::MAX] {
        let mut record = GOLDEN;
        set_word(&mut record, 16, 4);
        record[28..40].fill(0);
        set_word(&mut record, 24, code);
        assert_eq!(decode_exit(&record).err(), Some(AbiError::Exit));
    }
    for offset in [24, 32] {
        for code in [0, 4, u32::MAX] {
            let mut record = GOLDEN;
            set_word(&mut record, offset, code);
            assert_eq!(decode_exit(&record).err(), Some(AbiError::Exit));
        }
    }
}

#[test]
fn reason_incompatible_nonzero_fields_are_rejected() {
    for reason in [1, 2, 3, 4, 6] {
        let mut valid = GOLDEN;
        set_word(&mut valid, 16, reason);
        valid[24..40].fill(0);
        if reason == 4 {
            set_word(&mut valid, 24, 1);
        }
        assert!(decode_exit(&valid).is_ok());
        for offset in [24, 28, 32, 36] {
            if reason == 4 && offset == 24 {
                continue;
            }
            let mut malformed = valid;
            set_word(&mut malformed, offset, 1);
            assert_eq!(decode_exit(&malformed).err(), Some(AbiError::Exit));
        }
    }
}

#[test]
fn invalid_fault_widths_are_rejected_by_decode_and_atomic_encode() {
    for (access, tag, invalid_widths) in [
        (Access::Read, 1, &[0, 3, 5, 15, 16, u32::MAX][..]),
        (Access::Write, 2, &[0, 3, 5, 15, 16, u32::MAX][..]),
        (Access::Execute, 3, &[0, 16, u32::MAX][..]),
    ] {
        for length in invalid_widths {
            let mut malformed = GOLDEN;
            set_word(&mut malformed, 32, tag);
            set_word(&mut malformed, 36, *length);
            assert_eq!(decode_exit(&malformed).err(), Some(AbiError::Exit));
            let invalid = ExecutionExit {
                retired: 1,
                reason: ExitReason::MemoryFault {
                    fault: MemoryFault {
                        address: GuestAddress(0x1234_5678),
                        access,
                        reason: FaultReason::Permission,
                    },
                    length: *length,
                },
            };
            let mut output = [0xa5; 40];
            assert_eq!(encode_exit(&invalid, &mut output), Err(AbiError::Exit));
            assert_eq!(output, [0xa5; 40]);
        }
    }
}

#[test]
fn fault_reason_must_match_whether_address_range_overflows() {
    for (reason, detail) in [
        (FaultReason::Unmapped, 1),
        (FaultReason::Permission, 2),
        (FaultReason::AddressOverflow, 3),
    ] {
        for (address, length) in [(0, 4), (u32::MAX, 1), (u32::MAX - 3, 4), (u32::MAX, 4)] {
            let overflows = address as u64 + length as u64 > (1u64 << 32);
            let valid = overflows == (reason == FaultReason::AddressOverflow);
            let original = ExecutionExit {
                retired: 1,
                reason: ExitReason::MemoryFault {
                    fault: MemoryFault {
                        address: GuestAddress(address),
                        access: Access::Read,
                        reason,
                    },
                    length,
                },
            };
            let mut record = GOLDEN;
            set_word(&mut record, 24, detail);
            set_word(&mut record, 28, address);
            set_word(&mut record, 36, length);
            let mut output = [0xa5; 40];
            if valid {
                assert!(decode_exit(&record).is_ok());
                assert_roundtrip(&original);
            } else {
                assert_eq!(decode_exit(&record).err(), Some(AbiError::Exit));
                assert_eq!(encode_exit(&original, &mut output), Err(AbiError::Exit));
                assert_eq!(output, [0xa5; 40]);
            }
        }
    }
}

#[test]
fn wrong_length_takes_priority_over_invalid_fault_or_header() {
    let invalid = ExecutionExit {
        retired: 1,
        reason: ExitReason::MemoryFault {
            fault: MemoryFault {
                address: GuestAddress(0),
                access: Access::Read,
                reason: FaultReason::AddressOverflow,
            },
            length: 0,
        },
    };
    for length in [0, 39, 41] {
        let mut output = vec![0xa5; length];
        assert_eq!(encode_exit(&invalid, &mut output), Err(AbiError::Length));
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
    let mut malformed = GOLDEN;
    malformed[0] = 0;
    set_word(&mut malformed, 16, 0);
    assert_eq!(decode_exit(&malformed[..39]).err(), Some(AbiError::Length));
    let mut oversized = malformed.to_vec();
    oversized.push(0);
    assert_eq!(decode_exit(&oversized).err(), Some(AbiError::Length));
}
