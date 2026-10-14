use crate::{abi::AbiError, memory::Access};

pub(super) fn validate(error_code: u32, access: Access, length: u32) -> Result<(), AbiError> {
    let valid_length = match access {
        Access::Read | Access::Write => (1..=832).contains(&length),
        Access::Execute => (1..=15).contains(&length),
    };
    if error_code > 7 || (error_code & 2 != 0) != (access == Access::Write) || !valid_length {
        return Err(AbiError::Exit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::{header::write_u32, x86::*},
        cpu::{ExecutionExit, ExitReason},
        memory::GuestAddress,
    };

    #[test]
    fn literal_page_fault_packets_preserve_user_write_and_execute_metadata() {
        for (error_code, access, tag, length) in [
            (4, Access::Read, 1, 832),
            (7, Access::Write, 2, 4),
            (0, Access::Execute, 3, 15),
        ] {
            let exit = ExecutionExit {
                retired: 3,
                reason: ExitReason::PageFault {
                    error_code,
                    linear: GuestAddress(0xffff_ffff),
                    access,
                    length,
                },
            };
            let mut bytes = [0; EXIT_SIZE];
            let mut expected = [0; EXIT_SIZE];
            expected[..16].copy_from_slice(&[82, 51, 69, 88, 12, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
            for (index, value) in [23, 3, error_code, u32::MAX, tag, length]
                .into_iter()
                .enumerate()
            {
                expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
            }
            encode_exit_v12(&exit, &mut bytes).unwrap();
            assert_eq!(bytes, expected);
            assert_eq!(decode_exit(&expected), Ok(exit));
            bytes.fill(0xa5);
            assert_eq!(encode_exit_v11(&exit, &mut bytes), Err(AbiError::Exit));
            assert_eq!(bytes, [0xa5; EXIT_SIZE]);
            expected[4] = 11;
            assert_eq!(decode_exit(&expected), Err(AbiError::Exit));
        }
    }

    #[test]
    fn malformed_fault_codes_access_and_lengths_fail_without_publication() {
        for (error_code, access, length) in [
            (8, Access::Read, 4),
            (2, Access::Read, 4),
            (0, Access::Write, 4),
            (2, Access::Execute, 4),
            (0, Access::Read, 0),
            (0, Access::Read, 833),
            (0, Access::Execute, 16),
        ] {
            let exit = ExecutionExit {
                retired: 0,
                reason: ExitReason::PageFault {
                    error_code,
                    linear: GuestAddress(0x3000),
                    access,
                    length,
                },
            };
            let mut bytes = [0xa5; EXIT_SIZE];
            assert_eq!(encode_exit_v12(&exit, &mut bytes), Err(AbiError::Exit));
            assert_eq!(bytes, [0xa5; EXIT_SIZE]);
        }
        let mut bytes = [
            82, 51, 69, 88, 12, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0,
            0, 0, 48, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0,
        ];
        assert!(decode_exit(&bytes).is_ok());
        for (offset, value) in [(24, 8), (24, 2), (32, 0), (32, 4), (36, 0), (36, 833)] {
            let before = bytes;
            write_u32(&mut bytes, offset, value);
            assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
            bytes = before;
        }
    }
}
