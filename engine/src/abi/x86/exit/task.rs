use super::*;

#[test]
fn invalid_tss_requires_its_additive_exit_version_and_canonical_selector_error() {
    for error_code in [0, 1, 4, 5, 0x48, 0x49, 0xfffc, 0xfffd] {
        let exit = ExecutionExit {
            retired: 0,
            reason: ExitReason::InvalidTss { error_code },
        };
        let mut bytes = [0xa5; 40];
        assert_eq!(encode_exit_v9(&exit, &mut bytes), Err(AbiError::Exit));
        assert_eq!(bytes, [0xa5; 40]);
        encode_exit_v10(&exit, &mut bytes).unwrap();
        assert_eq!(decode_exit(&bytes), Ok(exit));
        assert_eq!(
            &bytes[..16],
            &[82, 51, 69, 88, 10, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(read_u32(&bytes, 16), 21);
        assert_eq!(read_u32(&bytes, 24), error_code);
        bytes[4..6].copy_from_slice(&9_u16.to_le_bytes());
        assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
    }
    for error_code in [2, 3, 0xfffe, 0xffff, 0x10000] {
        let mut bytes = [0xa5; 40];
        assert_eq!(
            encode_exit_v10(
                &ExecutionExit {
                    retired: 0,
                    reason: ExitReason::InvalidTss { error_code }
                },
                &mut bytes
            ),
            Err(AbiError::Exit)
        );
        assert_eq!(bytes, [0xa5; 40]);
    }
}

#[test]
fn unavailable_device_requires_version_eleven_without_details_or_memory_geometry() {
    let exit = ExecutionExit {
        retired: 2,
        reason: ExitReason::DeviceNotAvailable,
    };
    let mut bytes = [0xa5; EXIT_SIZE];
    assert_eq!(encode_exit_v10(&exit, &mut bytes), Err(AbiError::Exit));
    assert_eq!(bytes, [0xa5; EXIT_SIZE]);
    encode_exit_v11(&exit, &mut bytes).unwrap();
    assert_eq!(&bytes[..16], b"R3EX\x0b\0\x01\0\x28\0\0\0\0\0\0\0");
    assert_eq!(read_u32(&bytes, 16), 22);
    assert_eq!(decode_exit(&bytes), Ok(exit));
    for offset in [24, 28, 32, 36] {
        let mut malformed = bytes;
        malformed[offset] = 1;
        assert_eq!(decode_exit(&malformed), Err(AbiError::Exit));
    }
    bytes[4..6].copy_from_slice(&10_u16.to_le_bytes());
    assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
}
