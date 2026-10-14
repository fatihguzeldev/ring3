use super::*;
use crate::{
    abi::{
        cpu_helper::{CpuMemoryError, encode_cpu_result, encode_cpu_result_v8},
        memory_helper::CpuScalarOperation,
    },
    cpu::x86::VerifiedPageFault32,
    memory::{Access, FaultReason, MemoryError, MemoryFault},
};

fn literal(version: u16, fields: [u32; 6]) -> [u8; 40] {
    let mut output = [0; 40];
    output[..4].copy_from_slice(b"R3MH");
    output[4..6].copy_from_slice(&version.to_le_bytes());
    output[6..8].copy_from_slice(&1_u16.to_le_bytes());
    output[8..12].copy_from_slice(&40_u32.to_le_bytes());
    for (index, field) in fields.into_iter().enumerate() {
        output[16 + 4 * index..20 + 4 * index].copy_from_slice(&field.to_le_bytes());
    }
    output
}
fn pf(linear: u32, access: PagingAccess32, present: bool) -> CpuPagingError32 {
    CpuPagingError32::PageFault(VerifiedPageFault32::application(
        GuestAddress(linear),
        access,
        present,
    ))
}
fn memory(at: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(at),
        access,
        reason,
    })
}

#[test]
fn application_fault_packets_keep_presence_write_and_execute_bits() {
    for (access, present, code, wire_access) in [
        (PagingAccess32::Read, false, 4, 1),
        (PagingAccess32::Read, true, 5, 1),
        (PagingAccess32::Write, false, 6, 2),
        (PagingAccess32::Write, true, 7, 2),
        (PagingAccess32::Execute, false, 20, 3),
        (PagingAccess32::Execute, true, 21, 3),
    ] {
        let mut output = [0xa5; 40];
        encode_cpu_result(
            108,
            0,
            Err(CpuMemoryError::Paging(pf(0x1005, access, present))),
            &mut output,
        )
        .unwrap();
        assert_eq!(output, literal(15, [9, 0, code, 0x1005, wire_access, 108]));
    }
}

#[test]
fn scalar_faults_keep_wrapping_membership_and_write_intent() {
    let mut output = [0xa5; 40];
    encode_cpu_paging_scalar_result(
        GuestAddress(0xfffffffe),
        4,
        CpuScalarOperation::Modify,
        Err(pf(0, PagingAccess32::Write, true)),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, literal(15, [9, 0, 7, 0, 2, 4]));
    for (start, width, operation, error) in [
        (
            0x1000,
            4,
            CpuScalarOperation::Read,
            pf(0x1004, PagingAccess32::Read, false),
        ),
        (
            0x1000,
            4,
            CpuScalarOperation::Modify,
            pf(0x1000, PagingAccess32::Read, false),
        ),
        (
            0x1000,
            4,
            CpuScalarOperation::Read,
            pf(0x1000, PagingAccess32::Execute, false),
        ),
        (
            0xfffffffe,
            2,
            CpuScalarOperation::Modify,
            pf(0, PagingAccess32::Write, false),
        ),
        (
            0x1000,
            3,
            CpuScalarOperation::Read,
            pf(0x1000, PagingAccess32::Read, false),
        ),
    ] {
        output.fill(0xa5);
        assert!(
            encode_cpu_paging_scalar_result(
                GuestAddress(start),
                width,
                operation,
                Err(error),
                &mut output
            )
            .is_err()
        );
        assert_eq!(output, [0xa5; 40]);
    }
}

#[test]
fn generic_and_stack_geometry_failures_are_output_neutral() {
    let mut output = [0xa5; 40];
    assert!(
        encode_cpu_result(
            577,
            0,
            Err(CpuMemoryError::Paging(pf(0, PagingAccess32::Read, false))),
            &mut output
        )
        .is_err()
    );
    assert_eq!(output, [0xa5; 40]);
    assert!(
        encode_cpu_result_v8(
            833,
            0,
            Err(CpuMemoryError::Paging(pf(0, PagingAccess32::Read, false))),
            &mut output
        )
        .is_err()
    );
    assert_eq!(output, [0xa5; 40]);
    for error in [
        pf(0, PagingAccess32::Read, false),
        CpuPagingError32::Memory(MemoryError::Allocation),
    ] {
        assert!(encode_stack(4, error, &mut output).is_err());
        assert_eq!(output, [0xa5; 40]);
    }
    assert!(encode_stack(8, pf(0, PagingAccess32::Write, false), &mut output).is_err());
    assert_eq!(output, [0xa5; 40]);
    encode_stack(2, pf(0, PagingAccess32::Write, false), &mut output).unwrap();
    assert_eq!(output, literal(15, [9, 0, 6, 0, 2, 2]));
}

#[test]
fn generic_legacy_success_memory_and_nonpaging_errors_are_byte_identical() {
    type Encoder = fn(u32, u32, Result<(), CpuMemoryError>, &mut [u8]) -> Result<(), AbiError>;
    for (encode, version) in [
        (encode_cpu_result as Encoder, 7),
        (encode_cpu_result_v8 as Encoder, 8),
    ] {
        for (result, fields) in [
            (Ok(()), [0, 0x11223344, 0, 0, 0, 4]),
            (
                Err(CpuMemoryError::GeneralProtection(0)),
                [3, 0, 0, 0, 0, 4],
            ),
            (
                Err(CpuMemoryError::FloatingPoint(0x8081)),
                [4, 0, 0x8081, 0, 0, 4],
            ),
            (
                Err(CpuMemoryError::SimdFloatingPoint(0x1f81)),
                [5, 0, 0x1f81, 0, 0, 4],
            ),
            (
                Err(CpuMemoryError::SegmentNotPresent(0x48)),
                [6, 0, 0x48, 0, 0, 4],
            ),
            (Err(CpuMemoryError::StackSegment(0)), [7, 0, 0, 0, 0, 4]),
        ] {
            let mut bytes = [0xa5; 40];
            encode(4, 0x11223344, result, &mut bytes).unwrap();
            assert_eq!(bytes, literal(version, fields));
        }
        for (raw, fields) in [
            (
                memory(0xffff_ffff, Access::Read, FaultReason::Unmapped),
                [1, 0, 1, 0xffff_ffff, 1, 4],
            ),
            (
                memory(0, Access::Write, FaultReason::Permission),
                [1, 0, 2, 0, 2, 4],
            ),
            (
                memory(0xffff_fffe, Access::Read, FaultReason::AddressOverflow),
                [1, 0, 3, 0xffff_fffe, 1, 4],
            ),
            (MemoryError::VersionExhausted, [2, 0, 1, 0, 0, 4]),
            (MemoryError::InvalidRange, [2, 0, 2, 0, 0, 4]),
        ] {
            assert_eq!(
                CpuMemoryError::from(CpuPagingError32::Memory(raw)),
                CpuMemoryError::Memory(raw)
            );
            for error in [
                CpuMemoryError::Memory(raw),
                CpuMemoryError::Paging(CpuPagingError32::Memory(raw)),
            ] {
                let mut bytes = [0xa5; 40];
                encode(4, 123, Err(error), &mut bytes).unwrap();
                assert_eq!(bytes, literal(version, fields));
            }
        }
    }
}

#[test]
fn scalar_success_and_raw_memory_keep_exact_v10_bytes() {
    for width in [1, 2, 4] {
        for (operation, result, fields) in [
            (
                CpuScalarOperation::Read,
                Ok(0x7e),
                [0, 0x7e, 0, 0, 0, width],
            ),
            (CpuScalarOperation::Store, Ok(0), [0, 0, 0, 0, 0, width]),
            (
                CpuScalarOperation::Modify,
                Ok(0x7e),
                [0, 0x7e, 0, 0, 0, width],
            ),
            (
                CpuScalarOperation::Read,
                Err(CpuPagingError32::Memory(memory(
                    0xffff_ffff,
                    Access::Read,
                    FaultReason::Permission,
                ))),
                [1, 0, 2, 0xffff_ffff, 1, width],
            ),
            (
                CpuScalarOperation::Store,
                Err(CpuPagingError32::Memory(memory(
                    0xffff_ffff,
                    Access::Write,
                    FaultReason::Unmapped,
                ))),
                [1, 0, 1, 0xffff_ffff, 2, width],
            ),
            (
                CpuScalarOperation::Modify,
                Err(CpuPagingError32::Memory(memory(
                    0xffff_ffff,
                    Access::Write,
                    FaultReason::Permission,
                ))),
                [1, 0, 2, 0xffff_ffff, 2, width],
            ),
            (
                CpuScalarOperation::Modify,
                Err(CpuPagingError32::Memory(MemoryError::VersionExhausted)),
                [2, 0, 1, 0, 0, width],
            ),
        ] {
            let mut bytes = [0xa5; 40];
            encode_cpu_paging_scalar_result(
                GuestAddress(0xffff_ffff),
                width,
                operation,
                result,
                &mut bytes,
            )
            .unwrap();
            assert_eq!(bytes, literal(10, fields));
        }
    }
    for result in [
        Ok(256),
        Err(CpuPagingError32::Memory(memory(
            0xffff_ffff,
            Access::Read,
            FaultReason::AddressOverflow,
        ))),
    ] {
        let mut bytes = [0xa5; 40];
        assert!(
            encode_cpu_paging_scalar_result(
                GuestAddress(0xffff_ffff),
                1,
                CpuScalarOperation::Read,
                result,
                &mut bytes
            )
            .is_err()
        );
        assert_eq!(bytes, [0xa5; 40]);
    }
}
