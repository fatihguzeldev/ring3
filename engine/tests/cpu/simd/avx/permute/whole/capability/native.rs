use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, YMM_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};
const FORMS: [(u8, u8, u8, bool); 4] = [
    (2, 0x6d, 0x16, false),
    (2, 0x6d, 0x36, false),
    (3, 0xfd, 0, true),
    (3, 0xfd, 1, true),
];
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
fn encoding(form: (u8, u8, u8, bool), operand: u8) -> Vec<u8> {
    let (map, control, opcode, immediate) = form;
    let mut bytes = vec![0xc4, 0xe0 | map, control, opcode, operand];
    if immediate {
        bytes.push(0x1b);
    }
    bytes
}
#[test]
fn new_whole_vector_forms_stay_private_while_old_lane_local_forms_remain_admitted() {
    for form in FORMS {
        for operand in [0xcb, 0x08] {
            assert_eq!(
                decode_one(&code(&encoding(form, operand)), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Simd)
            );
        }
    }
    for bytes in [
        &[0xc4, 0xe2, 0x75, 0x0c, 0xc2][..],
        &[0xc4, 0xe3, 0x7d, 0x05, 0xc1, 0x1b][..],
    ] {
        assert!(decode_one(&code(bytes), GuestAddress(0x1000)).is_ok());
    }
}
#[test]
fn wrong_w_l_and_reserved_immediate_vvvv_refuse_before_private_gate() {
    for form in FORMS {
        let bytes = encoding(form, 0xcb);
        for changed in [bytes[2] ^ 0x80, bytes[2] & !4] {
            let mut invalid = bytes.clone();
            invalid[2] = changed;
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        if form.3 {
            let mut invalid = bytes.clone();
            invalid[2] &= !8;
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut invalid = vec![prefix];
            invalid.extend(&bytes);
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}
#[test]
fn whole_descriptors_refuse_before_reading_vector_state_or_address_metadata() {
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
    for (descriptor, operands, immediate) in [
        (0x0100041f, 1 | 2 << 3 | 3 << 6, 0),
        (0x0100041f, 1 | 2 << 3 | 1 << 12, 0),
        (0x01000420, 1 | 3 << 6, 255),
        (0x01000420, 1 | 1 << 12, 255),
    ] {
        let arena = engine.arena_mut().unwrap();
        arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
        arena[YMM_OFFSET + 12] = 1;
        let before = arena.to_vec();
        assert_eq!(
            engine.avx_execute(descriptor, operands, immediate, 0, 0, 2),
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
}
