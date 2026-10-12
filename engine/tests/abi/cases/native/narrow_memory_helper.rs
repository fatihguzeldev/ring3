use ring3_engine::abi::{
    AbiError,
    memory_helper::{NarrowReadWidth, encode_helper_result, encode_narrow_helper_result},
};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault};

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

fn encode(address: u32, width: NarrowReadWidth, result: Result<u32, MemoryError>) -> [u8; 40] {
    let mut bytes = [0xa5; 40];
    encode_narrow_helper_result(GuestAddress(address), width, result, &mut bytes).unwrap();
    bytes
}

fn fields(bytes: &[u8]) -> [u32; 6] {
    assert_eq!(
        &bytes[..16],
        &[82, 51, 77, 72, 2, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]
    );
    std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[16 + index * 4..20 + index * 4].try_into().unwrap())
    })
}

fn invalid(address: u32, width: NarrowReadWidth, result: Result<u32, MemoryError>) {
    let mut bytes = [0xa5; 40];
    assert_eq!(
        encode_narrow_helper_result(GuestAddress(address), width, result, &mut bytes),
        Err(AbiError::MemoryHelper)
    );
    assert_eq!(bytes, [0xa5; 40]);
}

#[test]
fn narrow_success_and_cross_page_fault_match_independent_literal_records() {
    assert_eq!(
        encode(u32::MAX, NarrowReadWidth::Byte, Ok(0xff)),
        [
            82, 51, 77, 72, 2, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
        ]
    );
    assert_eq!(
        encode(
            0x1fff,
            NarrowReadWidth::Word,
            Err(fault(0x2000, Access::Read, FaultReason::Permission))
        ),
        [
            82, 51, 77, 72, 2, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0,
            0, 0, 32, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0,
        ]
    );
}

#[test]
fn success_values_and_requests_obey_exact_byte_and_word_limits_atomically() {
    for (width, length, maximum, last_start) in [
        (NarrowReadWidth::Byte, 1, 0xff, u32::MAX),
        (NarrowReadWidth::Word, 2, 0xffff, u32::MAX - 1),
    ] {
        for value in [0, 1, maximum / 2, maximum] {
            assert_eq!(
                fields(&encode(last_start, width, Ok(value))),
                [0, value, 0, 0, 0, length]
            );
        }
        for value in [maximum + 1, u32::MAX] {
            invalid(0x1000, width, Ok(value));
        }
    }
    invalid(u32::MAX, NarrowReadWidth::Word, Ok(0));
}

#[test]
fn fault_bytes_are_contained_in_the_requested_span_including_terminal_byte() {
    for reason in [FaultReason::Unmapped, FaultReason::Permission] {
        let detail = if reason == FaultReason::Unmapped {
            1
        } else {
            2
        };
        for (request, width, length, at) in [
            (u32::MAX, NarrowReadWidth::Byte, 1, u32::MAX),
            (u32::MAX - 1, NarrowReadWidth::Word, 2, u32::MAX - 1),
            (u32::MAX - 1, NarrowReadWidth::Word, 2, u32::MAX),
            (0x1fff, NarrowReadWidth::Word, 2, 0x2000),
        ] {
            assert_eq!(
                fields(&encode(
                    request,
                    width,
                    Err(fault(at, Access::Read, reason))
                )),
                [1, 0, detail, at, 1, length]
            );
        }
    }
    assert_eq!(
        fields(&encode(
            u32::MAX,
            NarrowReadWidth::Word,
            Err(fault(u32::MAX, Access::Read, FaultReason::AddressOverflow))
        )),
        [1, 0, 3, u32::MAX, 1, 2]
    );
}

#[test]
fn wrong_access_fault_span_and_overflow_identity_preserve_output() {
    for (request, width, at, access, reason) in [
        (
            0x1000,
            NarrowReadWidth::Byte,
            0x1000,
            Access::Write,
            FaultReason::Unmapped,
        ),
        (
            0x1000,
            NarrowReadWidth::Word,
            0x1000,
            Access::Execute,
            FaultReason::Permission,
        ),
        (
            0x1000,
            NarrowReadWidth::Byte,
            0x1001,
            Access::Read,
            FaultReason::Permission,
        ),
        (
            0x1000,
            NarrowReadWidth::Word,
            0x0fff,
            Access::Read,
            FaultReason::Unmapped,
        ),
        (
            0x1000,
            NarrowReadWidth::Word,
            0x1002,
            Access::Read,
            FaultReason::Unmapped,
        ),
        (
            u32::MAX,
            NarrowReadWidth::Byte,
            u32::MAX,
            Access::Read,
            FaultReason::AddressOverflow,
        ),
        (
            u32::MAX - 1,
            NarrowReadWidth::Word,
            u32::MAX - 1,
            Access::Read,
            FaultReason::AddressOverflow,
        ),
        (
            u32::MAX,
            NarrowReadWidth::Word,
            u32::MAX,
            Access::Read,
            FaultReason::Permission,
        ),
        (
            u32::MAX,
            NarrowReadWidth::Word,
            0,
            Access::Read,
            FaultReason::AddressOverflow,
        ),
        (
            u32::MAX,
            NarrowReadWidth::Word,
            u32::MAX - 1,
            Access::Read,
            FaultReason::AddressOverflow,
        ),
    ] {
        invalid(request, width, Err(fault(at, access, reason)));
    }
}

#[test]
fn infrastructure_records_keep_width_even_for_arbitrary_request_and_v1_is_unchanged() {
    for (width, length) in [(NarrowReadWidth::Byte, 1), (NarrowReadWidth::Word, 2)] {
        for (error, detail) in [
            (MemoryError::VersionExhausted, 1),
            (MemoryError::Allocation, 2),
            (MemoryError::InvalidRange, 2),
        ] {
            assert_eq!(
                fields(&encode(u32::MAX, width, Err(error))),
                [2, 0, detail, 0, 0, length]
            );
        }
    }
    let mut legacy = [0xa5; 40];
    encode_helper_result(Ok(u32::MAX), &mut legacy).unwrap();
    assert_eq!(
        &legacy[..16],
        &[82, 51, 77, 72, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(&legacy[20..24], &[255; 4]);
    assert_eq!(&legacy[24..], &[0; 16]);
    encode_helper_result(
        Err(fault(0x1000, Access::Read, FaultReason::Permission)),
        &mut legacy,
    )
    .unwrap();
    assert_eq!(&legacy[36..], &[4, 0, 0, 0]);
}

#[test]
fn exact_output_length_has_priority_over_invalid_result_and_never_mutates() {
    for length in (0..40).chain([41, 80]) {
        let mut output = vec![0xa5; length];
        assert_eq!(
            encode_narrow_helper_result(
                GuestAddress(u32::MAX),
                NarrowReadWidth::Word,
                Err(fault(0, Access::Execute, FaultReason::AddressOverflow)),
                &mut output
            ),
            Err(AbiError::Length)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
}
