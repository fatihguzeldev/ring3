use ring3_engine::{
    abi::{
        AbiError,
        memory_helper::{
            BYTE_STORE_HELPER_VERSION, HELPER_SIZE, NarrowReadWidth, encode_byte_store_result,
            encode_helper_result, encode_narrow_helper_result,
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
};

fn packet(version: u16, fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 0, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

#[test]
fn byte_success_and_exact_write_faults_have_independent_v3_packets() {
    assert_eq!((HELPER_SIZE, BYTE_STORE_HELPER_VERSION), (40, 3));
    for address in [0, 0x1fff, u32::MAX] {
        let mut output = [0xa5; 40];
        encode_byte_store_result(GuestAddress(address), Ok(()), &mut output).unwrap();
        assert_eq!(output, packet(3, [0, 0, 0, 0, 0, 1]));
        for (reason, detail) in [(FaultReason::Unmapped, 1), (FaultReason::Permission, 2)] {
            encode_byte_store_result(
                GuestAddress(address),
                Err(fault(address, Access::Write, reason)),
                &mut output,
            )
            .unwrap();
            assert_eq!(output, packet(3, [1, 0, detail, address, 2, 1]));
        }
    }
}

#[test]
fn infrastructure_is_canonical_and_does_not_retain_request_or_old_value() {
    for (error, detail) in [
        (MemoryError::VersionExhausted, 1),
        (MemoryError::Allocation, 2),
        (MemoryError::Capacity, 2),
        (MemoryError::InvalidRange, 2),
        (
            MemoryError::AlreadyMapped {
                address: GuestAddress(0x1000),
            },
            2,
        ),
        (
            MemoryError::NotMapped {
                address: GuestAddress(0x2000),
            },
            2,
        ),
    ] {
        let mut output = [0xff; 40];
        encode_byte_store_result(GuestAddress(u32::MAX), Err(error), &mut output).unwrap();
        assert_eq!(output, packet(3, [2, 0, detail, 0, 0, 1]));
    }
}

#[test]
fn invalid_faults_and_length_priority_preserve_every_output_byte() {
    for error in [
        fault(0x1000, Access::Read, FaultReason::Unmapped),
        fault(0x1000, Access::Execute, FaultReason::Permission),
        fault(0x1000, Access::Write, FaultReason::AddressOverflow),
        fault(0x1001, Access::Write, FaultReason::Permission),
    ] {
        let mut output = [0xa5; 40];
        assert_eq!(
            encode_byte_store_result(GuestAddress(0x1000), Err(error), &mut output),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
    for length in (0..40).chain([41, 80]) {
        for result in [
            Ok(()),
            Err(fault(1, Access::Execute, FaultReason::AddressOverflow)),
        ] {
            let mut output = vec![0xa5; length];
            assert_eq!(
                encode_byte_store_result(GuestAddress(0), result, &mut output),
                Err(AbiError::Length)
            );
            assert_eq!(output, vec![0xa5; length]);
        }
    }
}

#[test]
fn old_store4_and_narrow_read_records_remain_separate_and_exact() {
    let mut output = [0xa5; 40];
    encode_helper_result(Ok(0x1122_3344), &mut output).unwrap();
    assert_eq!(output, packet(1, [0, 0x1122_3344, 0, 0, 0, 0]));
    encode_helper_result(
        Err(fault(0x2000, Access::Write, FaultReason::Permission)),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, packet(1, [1, 0, 2, 0x2000, 2, 4]));
    encode_narrow_helper_result(
        GuestAddress(u32::MAX),
        NarrowReadWidth::Byte,
        Ok(0xff),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, packet(2, [0, 0xff, 0, 0, 0, 1]));
    encode_byte_store_result(GuestAddress(u32::MAX), Ok(()), &mut output).unwrap();
    assert_eq!(output, packet(3, [0, 0, 0, 0, 0, 1]));
}
