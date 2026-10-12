use ring3_engine::abi::{
    AbiError,
    memory_helper::{HELPER_SIZE, encode_helper_result},
};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault};

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

fn assert_header(bytes: &[u8]) {
    assert_eq!(bytes.len(), 40);
    assert_eq!(
        &bytes[..16],
        &[0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn helper_success_has_canonical_header_value_and_zero_other_fields() {
    assert_eq!(HELPER_SIZE, 40);
    for value in [0, 0x1234_5678, u32::MAX] {
        let mut output = [0xa5; 40];
        encode_helper_result(Ok(value), &mut output).unwrap();
        assert_header(&output);
        assert_eq!(word(&output, 16), 0);
        assert_eq!(word(&output, 20), value);
        assert_eq!(&output[24..], &[0; 16]);
    }
}

#[test]
fn permission_write_fault_matches_independent_literal_wire_record() {
    let mut output = [0xa5; 40];
    encode_helper_result(
        Err(fault(0x1ffe, Access::Write, FaultReason::Permission)),
        &mut output,
    )
    .unwrap();
    assert_eq!(
        output,
        [
            0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2,
            0, 0, 0, 0xfe, 0x1f, 0, 0, 2, 0, 0, 0, 4, 0, 0, 0,
        ]
    );
}

#[test]
fn read_and_write_fault_codes_and_infrastructure_codes_are_explicit() {
    for (access, tag) in [(Access::Read, 1), (Access::Write, 2)] {
        for (reason, detail, address) in [
            (FaultReason::Unmapped, 1, 0x2000),
            (FaultReason::Permission, 2, 0x2000),
            (FaultReason::AddressOverflow, 3, 0xffff_fffd),
        ] {
            let mut output = [0xa5; 40];
            encode_helper_result(Err(fault(address, access, reason)), &mut output).unwrap();
            assert_header(&output);
            assert_eq!(
                [
                    word(&output, 16),
                    word(&output, 20),
                    word(&output, 24),
                    word(&output, 28),
                    word(&output, 32),
                    word(&output, 36)
                ],
                [1, 0, detail, address, tag, 4]
            );
        }
    }
    for (error, detail) in [
        (MemoryError::VersionExhausted, 1),
        (MemoryError::Allocation, 2),
        (MemoryError::Capacity, 2),
        (MemoryError::InvalidRange, 2),
    ] {
        let mut output = [0xa5; 40];
        encode_helper_result(Err(error), &mut output).unwrap();
        assert_header(&output);
        assert_eq!(
            [word(&output, 16), word(&output, 20), word(&output, 24)],
            [2, 0, detail]
        );
        assert_eq!(&output[28..], &[0; 12]);
    }
}

#[test]
fn invalid_helper_fault_and_wrong_lengths_preserve_output() {
    for error in [
        fault(0x1000, Access::Execute, FaultReason::Unmapped),
        fault(0xffff_fffd, Access::Read, FaultReason::Permission),
        fault(0, Access::Write, FaultReason::AddressOverflow),
    ] {
        let mut output = [0xa5; 40];
        assert_eq!(
            encode_helper_result(Err(error), &mut output),
            Err(AbiError::MemoryHelper)
        );
        assert_eq!(output, [0xa5; 40]);
    }
    for length in (0..40).chain([41, 80]) {
        let mut output = vec![0xa5; length];
        assert_eq!(
            encode_helper_result(
                Err(fault(0, Access::Execute, FaultReason::Unmapped)),
                &mut output
            ),
            Err(AbiError::Length)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
}
