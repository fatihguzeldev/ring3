use super::*;
use crate::cpu::x86::VerifiedPageFault32;
use crate::memory::MemoryError;

fn fault(linear: u32, access: PagingAccess32) -> CpuPagingError32 {
    CpuPagingError32::PageFault(VerifiedPageFault32::application(
        GuestAddress(linear),
        access,
        false,
    ))
}
fn result(error: CpuPagingError32) -> GatherResult {
    GatherResult {
        completed_lanes: 5,
        fault: Some(GatherFaultResult {
            lane: 3,
            kind: GatherFaultKind::Paging(error),
        }),
    }
}
fn geometry(width: u32) -> GatherPagingGeometry {
    GatherPagingGeometry {
        selected: 0b1101,
        lanes: 4,
        width,
        fault_start: GuestAddress(0xffff_fffe),
    }
}
fn literal(version: u16, fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"R3GH");
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&40_u32.to_le_bytes());
    for (index, value) in fields.into_iter().enumerate() {
        bytes[16 + 4 * index..20 + 4 * index].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn typed_gather_packets_keep_prefix_lane_and_retained_width_geometry() {
    for width in [4, 8] {
        let error = fault(0, PagingAccess32::Read);
        let fields = [9, 4, 0, 1, 5, 3];
        let mut output = [0xa5; 40];
        encode_gather_paging_result(result(error), width, &mut output).unwrap();
        assert_eq!(output, literal(2, fields));
        let parsed = decode_gather_paging_result(&output, geometry(width)).unwrap();
        assert_eq!((parsed.completed_lanes, parsed.fault_lane), (5, 3));
        assert!(parsed.agrees_with(error));
        assert!(decode_gather_address(&output).is_err());
    }
}

#[test]
fn old_codec_refuses_typed_paging_without_geometry_and_raw_packets_stay_v1() {
    let typed = result(fault(0, PagingAccess32::Read));
    let mut output = [0xa5; 40];
    assert_eq!(
        encode_gather_result(typed, &mut output),
        Err(AbiError::MemoryHelper)
    );
    assert_eq!(output, [0xa5; 40]);
    let raw = CpuPagingError32::Memory(MemoryError::Fault(MemoryFault {
        address: GuestAddress(0),
        access: Access::Read,
        reason: FaultReason::Unmapped,
    }));
    encode_gather_result(result(raw), &mut output).unwrap();
    assert_eq!(output, literal(1, [1, 1, 0, 1, 5, 3]));
    for error in [
        fault(0, PagingAccess32::Write),
        fault(0, PagingAccess32::Execute),
    ] {
        output.fill(0xa5);
        assert_eq!(
            encode_gather_paging_result(result(error), 8, &mut output),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
}

#[test]
fn parser_rejects_cross_version_status_and_impossible_progress_or_span_without_fault_authority() {
    let valid = literal(2, [9, 4, 0, 1, 5, 3]);
    for (offset, value) in [
        (4, 0x10001_u32),
        (4, 0x10003),
        (12, 1),
        (16, 0),
        (16, 1),
        (20, 6),
        (20, 0),
        (28, 2),
        (28, 0x101),
        (32, 4),
        (36, 2),
        (36, 4),
        (24, 2),
    ] {
        let mut bytes = valid;
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(decode_gather_paging_result(&bytes, geometry(4)).is_err());
        assert_eq!(valid, literal(2, [9, 4, 0, 1, 5, 3]));
    }
    for fields in [
        [8, 0, 0, 0x101, 5, 3],
        [8, 5, 0, 0x101, 5, 3],
        [8, 1, 0, 1, 5, 3],
    ] {
        assert!(decode_gather_paging_result(&literal(2, fields), geometry(8)).is_err());
    }
    for geometry in [
        GatherPagingGeometry {
            selected: 0,
            ..geometry(4)
        },
        GatherPagingGeometry {
            selected: 0x80,
            ..geometry(4)
        },
        GatherPagingGeometry {
            lanes: 3,
            ..geometry(4)
        },
        GatherPagingGeometry {
            width: 2,
            ..geometry(4)
        },
    ] {
        assert!(decode_gather_paging_result(&valid, geometry).is_err());
    }
}
