use ring3_engine::{
    abi::{
        AbiError,
        memory_helper::{
            HELPER_SIZE, NarrowReadWidth, WORD_EXCHANGE_HELPER_VERSION,
            encode_byte_exchange_result, encode_byte_store_result, encode_helper_result,
            encode_narrow_helper_result, encode_word_exchange_result, encode_word_store_result,
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
fn word_boundaries_and_read_first_faults_have_exact_width_two_v6_packets() {
    assert_eq!((HELPER_SIZE, WORD_EXCHANGE_HELPER_VERSION), (40, 6));
    for address in [0, 0x1fff, u32::MAX - 1] {
        for value in [0, 1, 0x7fff, 0x8000, 0xffff] {
            let mut output = [0xa5; 40];
            encode_word_exchange_result(GuestAddress(address), Ok(value), &mut output).unwrap();
            assert_eq!(output, packet(6, [0, value, 0, 0, 0, 2]));
        }
        for at in [address, address + 1]
            .into_iter()
            .filter(|&at| at < u32::MAX)
        {
            for (access, reason, detail, encoded_access) in [
                (Access::Read, FaultReason::Unmapped, 1, 1),
                (Access::Read, FaultReason::Permission, 2, 1),
                (Access::Write, FaultReason::Permission, 2, 2),
            ] {
                let mut output = [0xa5; 40];
                encode_word_exchange_result(
                    GuestAddress(address),
                    Err(fault(at, access, reason)),
                    &mut output,
                )
                .unwrap();
                assert_eq!(output, packet(6, [1, 0, detail, at, encoded_access, 2]));
            }
        }
    }
    let mut output = [0xa5; 40];
    encode_word_exchange_result(
        GuestAddress(u32::MAX),
        Err(fault(u32::MAX, Access::Read, FaultReason::AddressOverflow)),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, packet(6, [1, 0, 3, u32::MAX, 1, 2]));
}

#[test]
fn infrastructure_packets_clear_old_value_and_address_but_retain_word_width() {
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
        encode_word_exchange_result(GuestAddress(u32::MAX), Err(error), &mut output).unwrap();
        assert_eq!(output, packet(6, [2, 0, detail, 0, 0, 2]));
    }
}

#[test]
fn malformed_word_values_faults_and_lengths_leave_the_entire_output_untouched() {
    let malformed = [
        (0x1000, Ok(0x10000)),
        (0x1000, Ok(u32::MAX)),
        (u32::MAX, Ok(0)),
        (
            0x1000,
            Err(fault(0x1000, Access::Write, FaultReason::Unmapped)),
        ),
        (
            0x1000,
            Err(fault(0x1000, Access::Read, FaultReason::AddressOverflow)),
        ),
        (
            u32::MAX,
            Err(fault(u32::MAX, Access::Write, FaultReason::AddressOverflow)),
        ),
        (
            u32::MAX,
            Err(fault(u32::MAX, Access::Read, FaultReason::Permission)),
        ),
        (
            u32::MAX,
            Err(fault(0, Access::Read, FaultReason::AddressOverflow)),
        ),
        (
            0x1000,
            Err(fault(0x1000, Access::Execute, FaultReason::Permission)),
        ),
        (
            0x1000,
            Err(fault(0x1000, Access::Execute, FaultReason::Unmapped)),
        ),
        (
            0x1000,
            Err(fault(0x0fff, Access::Read, FaultReason::Permission)),
        ),
        (
            0x1000,
            Err(fault(0x1002, Access::Write, FaultReason::Permission)),
        ),
        (
            u32::MAX - 1,
            Err(fault(u32::MAX, Access::Read, FaultReason::Permission)),
        ),
        (
            u32::MAX - 1,
            Err(fault(u32::MAX, Access::Write, FaultReason::Permission)),
        ),
    ];
    for (address, result) in malformed {
        let mut output = [0xa5; 40];
        assert_eq!(
            encode_word_exchange_result(GuestAddress(address), result, &mut output),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
    for length in (0..40).chain([41, 80]) {
        for result in [Ok(0xffff), Ok(0x10000), Err(MemoryError::VersionExhausted)] {
            let mut output = vec![0xa5; length];
            assert_eq!(
                encode_word_exchange_result(GuestAddress(u32::MAX), result, &mut output),
                Err(AbiError::Length)
            );
            assert_eq!(output, vec![0xa5; length]);
        }
    }
}

#[test]
fn old_v1_through_v5_helper_packets_remain_byte_exact_and_distinct() {
    let mut output = [0xa5; 40];
    encode_helper_result(Ok(0x1122_3344), &mut output).unwrap();
    assert_eq!(output, packet(1, [0, 0x1122_3344, 0, 0, 0, 0]));
    encode_narrow_helper_result(
        GuestAddress(u32::MAX - 1),
        NarrowReadWidth::Word,
        Ok(0xffff),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, packet(2, [0, 0xffff, 0, 0, 0, 2]));
    encode_byte_store_result(GuestAddress(u32::MAX), Ok(()), &mut output).unwrap();
    assert_eq!(output, packet(3, [0, 0, 0, 0, 0, 1]));
    encode_word_store_result(GuestAddress(u32::MAX - 1), Ok(()), &mut output).unwrap();
    assert_eq!(output, packet(4, [0, 0, 0, 0, 0, 2]));
    encode_byte_exchange_result(GuestAddress(u32::MAX), Ok(0xff), &mut output).unwrap();
    assert_eq!(output, packet(5, [0, 0xff, 0, 0, 0, 1]));
}
