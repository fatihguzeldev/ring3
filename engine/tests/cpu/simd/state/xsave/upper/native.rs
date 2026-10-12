use ring3_engine::{
    abi::{
        AbiError,
        cpu_helper::{encode_cpu_result, encode_cpu_result_v8},
        x86::{
            EXIT_SIZE, decode_exit, encode_exit_v6, encode_exit_v7, encode_exit_v8, encode_exit_v9,
        },
    },
    cpu::{ExecutionExit, ExitReason, x86::xstate},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
};
#[test]
fn explicit_xstate_protocol_versions_do_not_widen_legacy_codecs() {
    for length in [577, 704, 832] {
        let mut bytes = [0x9b; 40];
        let before = bytes;
        assert_eq!(
            encode_cpu_result(length, 0, Ok(()), &mut bytes),
            Err(AbiError::Length)
        );
        assert_eq!(bytes, before);
        encode_cpu_result_v8(length, 0, Ok(()), &mut bytes).unwrap();
        assert_eq!(&bytes[..8], b"R3MH\x08\0\x01\0");
        assert_eq!(
            u32::from_le_bytes(bytes[36..40].try_into().unwrap()),
            length
        );
    }
    let mut bytes = [0x9b; 40];
    let before = bytes;
    assert_eq!(
        encode_cpu_result_v8(833, 0, Ok(()), &mut bytes),
        Err(AbiError::Length)
    );
    assert_eq!(bytes, before);
    for access in [Access::Read, Access::Write] {
        let exit = ExecutionExit {
            retired: 0,
            reason: ExitReason::MemoryFault {
                fault: MemoryFault {
                    address: GuestAddress(0x6000),
                    access,
                    reason: FaultReason::Permission,
                },
                length: 832,
            },
        };
        let mut bytes = [0x9b; EXIT_SIZE];
        let before = bytes;
        for encode in [encode_exit_v6, encode_exit_v7, encode_exit_v8] {
            assert_eq!(encode(&exit, &mut bytes), Err(AbiError::Exit));
            assert_eq!(bytes, before);
        }
        encode_exit_v9(&exit, &mut bytes).unwrap();
        assert_eq!(decode_exit(&bytes), Ok(exit));
        for version in [1_u16, 2, 3, 4, 5, 6, 7, 8] {
            bytes[4..6].copy_from_slice(&version.to_le_bytes());
            assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
        }
        bytes[4..6].copy_from_slice(&9_u16.to_le_bytes());
        for length in [0_u32, 833] {
            bytes[36..40].copy_from_slice(&length.to_le_bytes());
            assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
        }
        bytes[32..36].copy_from_slice(&3_u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&15_u32.to_le_bytes());
        assert!(decode_exit(&bytes).is_ok());
        bytes[36..40].copy_from_slice(&16_u32.to_le_bytes());
        assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
    }
    assert_eq!(xstate::read_control(0), Ok(3));
    assert_eq!(xstate::requested(u32::MAX, u32::MAX), 3);
    assert_eq!(xstate::upper::requested(u32::MAX, u32::MAX), 7);
}
