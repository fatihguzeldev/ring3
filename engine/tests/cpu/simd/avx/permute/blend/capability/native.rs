use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{SIMD_SIZE, YMM_SIZE, encode_simd, encode_ymm},
    },
    cpu::{
        UnsupportedFeature,
        x86::{
            decode::{DecodeError, decode_one},
            simd::{SimdState, ymm::YmmUpperState},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, StoreCompletion},
};
fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}
#[test]
fn only_integer_alias_encodings_remain_private_after_strict_encoding_checks() {
    for control in [0x69, 0x6d] {
        for operand in [0xcb, 0x08] {
            let bytes = [0xc4, 0xe3, control, 2, operand, 0xaa];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Simd)
            );
            let mut floating = bytes;
            floating[3] = 0x0c;
            assert!(decode_one(&code(&floating), GuestAddress(0x1000)).is_ok());
            let mut invalid = bytes;
            invalid[2] |= 0x80;
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut invalid = vec![prefix];
                invalid.extend(bytes);
                assert_eq!(
                    decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
        }
    }
}
fn words(values: [u32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (lane, value) in values.into_iter().enumerate() {
        bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
#[test]
fn shared_avx1_helpers_remain_available_with_full_frame_and_ram_oracles() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let mut before_ram = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();
    for wide in [false, true] {
        let mut simd = SimdState {
            mxcsr: 0,
            ..Default::default()
        };
        simd.xmm[1] = words([0x7f800001, 0x80000000, 1, 0xdeadbeef]);
        simd.xmm[2] = words([0x7fc01234, 0xff800001, 0x3f800000, 0x12345678]);
        let mut upper = YmmUpperState::default();
        upper.registers[1] = [0x11; 16];
        upper.registers[2] = [0x22; 16];
        let arena = engine.arena_mut().unwrap();
        encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
        encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
        arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
        let mut expected = arena.to_vec();
        simd.xmm[0] = words([0x7f800001, 0xff800001, 1, 0x12345678]);
        upper.registers[0] = if wide {
            words([0x11111111, 0x22222222, 0x11111111, 0x22222222])
        } else {
            [0; 16]
        };
        encode_simd(&simd, &mut expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
        encode_ymm(&upper, &mut expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
        expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
            .copy_from_slice(&2_u32.to_le_bytes());
        let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
        packet.fill(0);
        packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
        assert_eq!(
            engine.avx_execute(
                if wide { 0x01000416 } else { 0x01000415 },
                1 << 3 | 2 << 6,
                0xaa,
                0,
                0,
                2
            ),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected);
        let mut after_ram = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut after_ram)
            .unwrap();
        assert_eq!(after_ram, before_ram);
        assert_eq!(engine.arena(), expected);
    }
}
