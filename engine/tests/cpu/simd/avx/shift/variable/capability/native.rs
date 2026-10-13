use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, YMM_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};
const FORMS: [(u8, u8, u32); 5] = [
    (0x45, 0, 19),
    (0x45, 0x80, 20),
    (0x46, 0, 21),
    (0x47, 0, 22),
    (0x47, 0x80, 23),
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
#[test]
fn both_lengths_of_element_shift_encodings_remain_outside_public_avx2() {
    for (opcode, w, _) in FORMS {
        for l in [0, 4] {
            for operand in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe2, 0x69 | w | l, opcode, operand];
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd)
                );
            }
        }
    }
    assert!(decode_one(&code(&[0xc5, 0xe9, 0xd2, 0xcb]), GuestAddress(0x1000)).is_ok());
}
#[test]
fn forbidden_prefixes_refuse_before_element_capability_gating() {
    for (opcode, w, _) in FORMS {
        for l in [0, 4] {
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let bytes = [prefix, 0xc4, 0xe2, 0x69 | w | l, opcode, 0xcb];
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
        }
    }
}
#[test]
fn canonical_element_descriptors_refuse_before_vector_or_address_reads() {
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
    for (_, _, kind) in FORMS {
        for bit in [0, 0x80] {
            for operands in [1 | 2 << 3 | 3 << 6, 1 | 2 << 3 | 1 << 12] {
                let arena = engine.arena_mut().unwrap();
                arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
                arena[YMM_OFFSET + 12] = 1;
                let before = arena.to_vec();
                assert_eq!(
                    engine.avx_execute(0x01000700 | kind | bit, operands, 0, 0, 0, 2),
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
    }
}
