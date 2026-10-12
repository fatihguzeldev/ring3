use ring3_engine::{
    abi::{
        AbiError,
        memory_helper::{HELPER_SIZE, WORD_STORE_HELPER_VERSION, encode_word_store_result},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
};

fn packet(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 4, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
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
fn word_success_and_each_span_fault_have_literal_v4_packets() {
    assert_eq!((HELPER_SIZE, WORD_STORE_HELPER_VERSION), (40, 4));
    for start in [0, 0x1fff, u32::MAX - 1] {
        let mut output = [0xa5; 40];
        encode_word_store_result(GuestAddress(start), Ok(()), &mut output).unwrap();
        assert_eq!(output, packet([0, 0, 0, 0, 0, 2]));
        for at in [start, start + 1] {
            for (reason, detail) in [(FaultReason::Unmapped, 1), (FaultReason::Permission, 2)] {
                encode_word_store_result(
                    GuestAddress(start),
                    Err(fault(at, Access::Write, reason)),
                    &mut output,
                )
                .unwrap();
                assert_eq!(output, packet([1, 0, detail, at, 2, 2]));
            }
        }
    }
    let mut output = [0xa5; 40];
    encode_word_store_result(
        GuestAddress(u32::MAX),
        Err(fault(u32::MAX, Access::Write, FaultReason::AddressOverflow)),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, packet([1, 0, 3, u32::MAX, 2, 2]));
}

#[test]
fn malformed_records_are_rejected_before_any_output_mutation() {
    for (start, result) in [
        (u32::MAX, Ok(())),
        (
            0x1000,
            Err(fault(0x1000, Access::Read, FaultReason::Unmapped)),
        ),
        (
            0x1000,
            Err(fault(0x1001, Access::Execute, FaultReason::Permission)),
        ),
        (
            0x1000,
            Err(fault(0x0fff, Access::Write, FaultReason::Unmapped)),
        ),
        (
            0x1000,
            Err(fault(0x1002, Access::Write, FaultReason::Permission)),
        ),
        (
            0x1000,
            Err(fault(0x1000, Access::Write, FaultReason::AddressOverflow)),
        ),
        (
            u32::MAX,
            Err(fault(u32::MAX, Access::Write, FaultReason::Permission)),
        ),
        (
            u32::MAX,
            Err(fault(0, Access::Write, FaultReason::AddressOverflow)),
        ),
    ] {
        let mut output = [0xa5; 40];
        assert_eq!(
            encode_word_store_result(GuestAddress(start), result, &mut output),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
    for length in (0..40).chain([41, 80]) {
        let mut output = vec![0xa5; length];
        assert_eq!(
            encode_word_store_result(GuestAddress(u32::MAX), Ok(()), &mut output),
            Err(AbiError::Length)
        );
        assert_eq!(output, vec![0xa5; length]);
    }
}

#[test]
fn infrastructure_records_clear_request_and_value_residue() {
    for (error, detail) in [
        (MemoryError::VersionExhausted, 1),
        (MemoryError::Allocation, 2),
        (MemoryError::Capacity, 2),
        (MemoryError::InvalidRange, 2),
    ] {
        let mut output = [0xff; 40];
        encode_word_store_result(GuestAddress(u32::MAX), Err(error), &mut output).unwrap();
        assert_eq!(output, packet([2, 0, detail, 0, 0, 2]));
    }
}
