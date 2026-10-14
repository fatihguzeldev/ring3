use super::*;
use crate::memory::{GuestAddress, MemoryFault};

fn literal(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 13, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (i, v) in fields.into_iter().enumerate() {
        bytes[16 + i * 4..20 + i * 4].copy_from_slice(&v.to_le_bytes());
    }
    bytes
}

#[test]
fn task_fault_and_missing_authority_packets_preserve_exact_phase_and_selector_bits() {
    for (result, fields) in [
        (TaskReturnAdmission::InvalidTss(0), [3, 0, 0, 0, 0, 0]),
        (TaskReturnAdmission::InvalidTss(4), [3, 0, 4, 0, 0, 0]),
        (
            TaskReturnAdmission::InvalidTss(0xfffc),
            [3, 0, 0xfffc, 0, 0, 0],
        ),
        (TaskReturnAdmission::NotPresent(0x48), [6, 0, 0x48, 0, 0, 0]),
        (TaskReturnAdmission::MissingAuthority, [8, 0, 1, 0, 0, 0]),
        (
            TaskReturnAdmission::Infrastructure(MemoryError::VersionExhausted),
            [2, 0, 1, 0, 0, 2],
        ),
    ] {
        let mut out = [0xa5; 40];
        encode_task_return_result(None, &result, &mut out).unwrap();
        assert_eq!(out, literal(fields));
    }
}

#[test]
fn wrapping_backlink_fault_is_bound_to_the_supplied_two_byte_source() {
    let fault = MemoryFault {
        address: GuestAddress(0),
        access: Access::Read,
        reason: FaultReason::Unmapped,
    };
    let result = TaskReturnAdmission::NeedsPagingAuthority { fault, length: 2 };
    let mut out = [0xa5; 40];
    encode_task_return_result(Some(u32::MAX), &result, &mut out).unwrap();
    assert_eq!(out, literal([1, 0, 1, 0, 1, 2]));
    for base in [None, Some(1), Some(u32::MAX - 1)] {
        out.fill(0xa5);
        assert_eq!(
            encode_task_return_result(base, &result, &mut out),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(out, [0xa5; 40]);
    }
}

#[test]
fn malformed_fault_geometry_or_selector_codes_leave_output_untouched() {
    let fault = MemoryFault {
        address: GuestAddress(0x5000),
        access: Access::Read,
        reason: FaultReason::Permission,
    };
    for result in [
        TaskReturnAdmission::InvalidTss(1),
        TaskReturnAdmission::InvalidTss(2),
        TaskReturnAdmission::InvalidTss(0x10000),
        TaskReturnAdmission::NotPresent(0),
        TaskReturnAdmission::NotPresent(4),
        TaskReturnAdmission::NotPresent(0x4b),
        TaskReturnAdmission::NeedsPagingAuthority { fault, length: 1 },
        TaskReturnAdmission::NeedsPagingAuthority {
            fault: MemoryFault {
                access: Access::Write,
                ..fault
            },
            length: 2,
        },
        TaskReturnAdmission::NeedsPagingAuthority {
            fault: MemoryFault {
                reason: FaultReason::AddressOverflow,
                ..fault
            },
            length: 2,
        },
        TaskReturnAdmission::Infrastructure(MemoryError::Fault(fault)),
    ] {
        let mut out = [0xa5; 40];
        assert!(encode_task_return_result(Some(0x5000), &result, &mut out).is_err());
        assert_eq!(out, [0xa5; 40]);
    }
}
