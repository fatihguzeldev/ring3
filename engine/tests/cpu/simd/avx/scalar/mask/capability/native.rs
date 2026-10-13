use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, YMM_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
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
fn wide_byte_masks_refuse_while_xmm_masks_remain_available() {
    for destination in 0..8 {
        for source in 0..8 {
            for w in [0, 0x80] {
                let mut bytes = [0xc4, 0xe1, 0x7d | w, 0xd7, 0xc0 | destination << 3 | source];
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd)
                );
                bytes[2] &= !4;
                assert!(decode_one(&code(&bytes), GuestAddress(0x1000)).is_ok());
            }
        }
    }
}

#[test]
fn malformed_mask_encoding_precedes_the_public_capability_refusal() {
    for width in [0x79, 0x7d] {
        let valid = [0xc4, 0xe1, width, 0xd7, 0xc1];
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, valid[0], valid[1], valid[2], valid[3], valid[4]];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        for reserved in 0..15 {
            let mut bytes = valid;
            bytes[2] = bytes[2] & !0x78 | reserved << 3;
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        let mut memory = valid;
        memory[4] = 0;
        assert_eq!(
            decode_one(&code(&memory), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn wide_mask_helper_refuses_before_state_or_stale_address_metadata() {
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
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
    arena[YMM_OFFSET + 12] = 1;
    let before = arena.to_vec();
    assert_eq!(
        engine.avx_execute(0x01000983, 1 | 3 << 6, 0, 0, 0, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    let mut after_ram = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut after_ram)
        .unwrap();
    assert_eq!(after_ram, before_ram);
    assert_eq!(engine.arena(), before);
}
