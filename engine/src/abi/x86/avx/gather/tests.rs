use super::*;
use crate::{
    abi::cpu_helper::{CpuMemoryError, encode_cpu_result},
    memory::GuestAddress,
};

fn request(segment: u32, offset: u32) -> [u8; 24] {
    let mut bytes = [0; 24];
    bytes[..12].copy_from_slice(&[b'R', b'3', b'G', b'A', 1, 0, 1, 0, 24, 0, 0, 0]);
    bytes[16..20].copy_from_slice(&segment.to_le_bytes());
    bytes[20..24].copy_from_slice(&offset.to_le_bytes());
    bytes
}

#[test]
fn request_keeps_modular_base_bits_and_strict_versioned_segment_authority() {
    for (index, segment) in [
        SegmentRegister::Es,
        SegmentRegister::Cs,
        SegmentRegister::Ss,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ]
    .into_iter()
    .enumerate()
    {
        for offset in [0, 0x2000, 0xfffffff0, u32::MAX] {
            assert_eq!(
                decode_gather_address(&request(index as u32, offset)),
                Ok(GatherRequest {
                    segment,
                    base_offset: offset
                })
            );
        }
    }
    for index in [6, 7, u32::MAX] {
        assert_eq!(
            decode_gather_address(&request(index, 0)),
            Err(AbiError::Reserved)
        );
    }
    let valid = request(3, 0);
    for (offset, changed, error) in [
        (0, b'M', AbiError::Magic),
        (4, 0, AbiError::Version),
        (4, 2, AbiError::Version),
        (5, 1, AbiError::Version),
        (6, 2, AbiError::Profile),
        (8, 40, AbiError::Length),
        (12, 1, AbiError::Reserved),
    ] {
        let mut bad = valid;
        bad[offset] = changed;
        assert_eq!(decode_gather_address(&bad), Err(error));
    }
    for length in 0..24 {
        assert_eq!(
            decode_gather_address(&valid[..length]),
            Err(AbiError::Length)
        );
    }
}

#[test]
fn result_has_a_dedicated_header_prefix_and_precise_element_fault_fields() {
    let outcomes = [
        (None, [0, 0, 0, 0, 5, u32::MAX]),
        (Some(GatherFaultKind::GeneralProtection), [3, 0, 0, 0, 5, 3]),
        (Some(GatherFaultKind::StackSegment), [7, 0, 0, 0, 5, 3]),
        (
            Some(GatherFaultKind::Memory(MemoryFault {
                address: GuestAddress(0x6000),
                access: Access::Read,
                reason: FaultReason::Unmapped,
            })),
            [1, 1, 0x6000, 1, 5, 3],
        ),
        (
            Some(GatherFaultKind::Memory(MemoryFault {
                address: GuestAddress(0x6000),
                access: Access::Read,
                reason: FaultReason::Permission,
            })),
            [1, 2, 0x6000, 1, 5, 3],
        ),
        (
            Some(GatherFaultKind::Memory(MemoryFault {
                address: GuestAddress(u32::MAX - 3),
                access: Access::Read,
                reason: FaultReason::AddressOverflow,
            })),
            [1, 3, u32::MAX - 3, 1, 5, 3],
        ),
    ];
    for (kind, fields) in outcomes {
        let mut output = [0xa5; 40];
        encode_gather_result(
            GatherResult {
                completed_lanes: 5,
                fault: kind.map(|kind| GatherFaultResult { lane: 3, kind }),
            },
            &mut output,
        )
        .unwrap();
        let mut expected = [0; 40];
        expected[..12].copy_from_slice(&[b'R', b'3', b'G', b'H', 1, 0, 1, 0, 40, 0, 0, 0]);
        for (index, value) in fields.into_iter().enumerate() {
            expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(output, expected);
        assert_eq!(
            crate::abi::header::validate_versions(&output, *b"R3MH", 40, &[7, 8]),
            Err(AbiError::Magic)
        );
    }
    let mut legacy = [0; 40];
    encode_cpu_result(4, 0, Err(CpuMemoryError::GeneralProtection(0)), &mut legacy).unwrap();
    assert_eq!(
        crate::abi::header::validate_versions(&legacy, *b"R3GH", 40, &[1]),
        Err(AbiError::Magic)
    );
}

#[test]
fn impossible_fault_shapes_are_rejected_before_output_mutation() {
    for (lane, completed, kind) in [
        (8, 0, GatherFaultKind::GeneralProtection),
        (0, 1, GatherFaultKind::StackSegment),
        (2, 4, GatherFaultKind::GeneralProtection),
        (7, 0x80, GatherFaultKind::GeneralProtection),
        (
            1,
            0,
            GatherFaultKind::Memory(MemoryFault {
                address: GuestAddress(0),
                access: Access::Write,
                reason: FaultReason::Unmapped,
            }),
        ),
        (
            1,
            0,
            GatherFaultKind::Memory(MemoryFault {
                address: GuestAddress(0),
                access: Access::Execute,
                reason: FaultReason::Permission,
            }),
        ),
    ] {
        let mut output = [0xa5; 40];
        assert_eq!(
            encode_gather_result(
                GatherResult {
                    completed_lanes: completed,
                    fault: Some(GatherFaultResult { lane, kind })
                },
                &mut output
            ),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
    for length in [0, 39, 41] {
        let mut output = vec![0xa5; length];
        assert_eq!(
            encode_gather_result(
                GatherResult {
                    completed_lanes: 0,
                    fault: None
                },
                &mut output
            ),
            Err(AbiError::Length)
        );
        assert_eq!(output, vec![0xa5; length]);
    }
}
