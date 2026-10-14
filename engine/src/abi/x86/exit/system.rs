use super::*;

#[test]
fn system_entry_has_a_versioned_exact_vector_record() {
    for vector in [0x29, 0x2e] {
        let exit = ExecutionExit {
            retired: 3,
            reason: ExitReason::SystemEntry { vector },
        };
        let mut packet = [0xa5; EXIT_SIZE];
        encode_exit_v14(&exit, &mut packet).unwrap();
        let mut expected = [0; EXIT_SIZE];
        expected[..16].copy_from_slice(&[82, 51, 69, 88, 14, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
        write_u32(&mut expected, REASON_OFFSET, 24);
        write_u32(&mut expected, RETIRED_OFFSET, 3);
        write_u32(&mut expected, DETAIL_OFFSET, u32::from(vector));
        assert_eq!(packet, expected);
        assert_eq!(decode_exit(&packet), Ok(exit));

        for version in ABI_VERSION..EXIT_VERSION_14 {
            let mut untouched = [0xa5; EXIT_SIZE];
            assert_eq!(encode(&exit, &mut untouched, version), Err(AbiError::Exit));
            assert_eq!(untouched, [0xa5; EXIT_SIZE]);
            let mut downgraded = packet;
            downgraded[4..6].copy_from_slice(&version.to_le_bytes());
            assert_eq!(decode_exit(&downgraded), Err(AbiError::Exit));
        }
    }
}

#[test]
fn system_entry_rejects_non_vector_and_memory_metadata() {
    let exit = ExecutionExit {
        retired: 1,
        reason: ExitReason::SystemEntry { vector: 0x2e },
    };
    let mut valid = [0; EXIT_SIZE];
    encode_exit_v14(&exit, &mut valid).unwrap();
    for (offset, value) in [
        (DETAIL_OFFSET, 0),
        (DETAIL_OFFSET, 4),
        (DETAIL_OFFSET, 0x80),
        (DETAIL_OFFSET, 0xff),
        (DETAIL_OFFSET, 0x100),
        (DETAIL_OFFSET, u32::MAX),
        (FAULT_ADDRESS_OFFSET, 1),
        (ACCESS_OFFSET, 1),
        (ACCESS_LENGTH_OFFSET, 1),
    ] {
        let mut malformed = valid;
        write_u32(&mut malformed, offset, value);
        assert_eq!(decode_exit(&malformed), Err(AbiError::Exit));
    }
    let mut unknown = valid;
    unknown[4..6].copy_from_slice(&15_u16.to_le_bytes());
    assert_eq!(decode_exit(&unknown), Err(AbiError::Version));
    for vector in [0, 4, 0x80, u8::MAX] {
        let unsupported = ExecutionExit {
            reason: ExitReason::SystemEntry { vector },
            ..exit
        };
        let mut untouched = [0xa5; EXIT_SIZE];
        assert_eq!(
            encode_exit_v14(&unsupported, &mut untouched),
            Err(AbiError::Exit)
        );
        assert_eq!(untouched, [0xa5; EXIT_SIZE]);
    }
}

#[test]
fn version_fourteen_preserves_existing_stop_contracts() {
    for reason in [
        ExitReason::NeedCode,
        ExitReason::Overflow,
        ExitReason::GeneralProtection { error_code: 0x402 },
        ExitReason::PageFault {
            error_code: 21,
            linear: GuestAddress(0x1000),
            access: Access::Execute,
            length: 1,
        },
    ] {
        let exit = ExecutionExit { retired: 0, reason };
        let mut packet = [0; EXIT_SIZE];
        encode_exit_v14(&exit, &mut packet).unwrap();
        assert_eq!(decode_exit(&packet), Ok(exit));
    }
}
