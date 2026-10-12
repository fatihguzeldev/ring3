use ring3_engine::{
    abi::{
        AbiError,
        memory_helper::{
            BYTE_EXCHANGE_HELPER_VERSION, HELPER_SIZE, NarrowReadWidth,
            encode_byte_exchange_result, encode_byte_store_result, encode_helper_result,
            encode_narrow_helper_result, encode_word_store_result,
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
fn every_old_byte_and_read_first_fault_has_an_exact_width_one_v5_packet() {
    assert_eq!((HELPER_SIZE, BYTE_EXCHANGE_HELPER_VERSION), (40, 5));
    for address in [0, 0x1fff, u32::MAX] {
        for value in 0..=255 {
            let mut output = [0xa5; 40];
            encode_byte_exchange_result(GuestAddress(address), Ok(value), &mut output).unwrap();
            assert_eq!(output, packet(5, [0, value, 0, 0, 0, 1]));
        }
        for (access, reason, detail, encoded_access) in [
            (Access::Read, FaultReason::Unmapped, 1, 1),
            (Access::Read, FaultReason::Permission, 2, 1),
            (Access::Write, FaultReason::Permission, 2, 2),
        ] {
            let mut output = [0xa5; 40];
            encode_byte_exchange_result(
                GuestAddress(address),
                Err(fault(address, access, reason)),
                &mut output,
            )
            .unwrap();
            assert_eq!(
                output,
                packet(5, [1, 0, detail, address, encoded_access, 1])
            );
        }
    }
}

#[test]
fn infrastructure_packets_clear_value_and_address_without_losing_byte_width() {
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
        encode_byte_exchange_result(GuestAddress(u32::MAX), Err(error), &mut output).unwrap();
        assert_eq!(output, packet(5, [2, 0, detail, 0, 0, 1]));
    }
}

#[test]
fn malformed_values_faults_and_lengths_leave_the_entire_output_untouched() {
    for address in [0, 0x1fff, u32::MAX] {
        let malformed = [
            Ok(256),
            Ok(u32::MAX),
            Err(fault(address, Access::Write, FaultReason::Unmapped)),
            Err(fault(address, Access::Read, FaultReason::AddressOverflow)),
            Err(fault(address, Access::Write, FaultReason::AddressOverflow)),
            Err(fault(address, Access::Execute, FaultReason::Permission)),
            Err(fault(address, Access::Execute, FaultReason::Unmapped)),
            Err(fault(
                address.wrapping_add(1),
                Access::Read,
                FaultReason::Permission,
            )),
            Err(fault(
                address.wrapping_sub(1),
                Access::Write,
                FaultReason::Permission,
            )),
        ];
        for result in malformed {
            let mut output = [0xa5; 40];
            assert_eq!(
                encode_byte_exchange_result(GuestAddress(address), result, &mut output),
                Err(AbiError::MemoryHelper)
            );
            assert_eq!(output, [0xa5; 40]);
        }
    }
    for length in (0..40).chain([41, 80]) {
        for result in [Ok(0xff), Ok(256), Err(MemoryError::VersionExhausted)] {
            let mut output = vec![0xa5; length];
            assert_eq!(
                encode_byte_exchange_result(GuestAddress(u32::MAX), result, &mut output),
                Err(AbiError::Length)
            );
            assert_eq!(output, vec![0xa5; length]);
        }
    }
}

#[test]
fn old_dword_read_byte_store_and_word_store_packets_remain_distinct() {
    let mut output = [0xa5; 40];
    encode_helper_result(Ok(0x1122_3344), &mut output).unwrap();
    assert_eq!(output, packet(1, [0, 0x1122_3344, 0, 0, 0, 0]));
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
    encode_word_store_result(GuestAddress(u32::MAX - 1), Ok(()), &mut output).unwrap();
    assert_eq!(output, packet(4, [0, 0, 0, 0, 0, 2]));
}
