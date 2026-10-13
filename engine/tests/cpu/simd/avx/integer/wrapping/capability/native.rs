use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};

const FORMS: [(u8, u32); 8] = [
    (0xfc, 26),
    (0xfd, 27),
    (0xfe, 28),
    (0xd4, 7),
    (0xf8, 22),
    (0xf9, 23),
    (0xfa, 24),
    (0xfb, 25),
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

fn ram(engine: &mut EngineInstance) -> Vec<u8> {
    let saved = engine.arena().to_vec();
    let mut value = Vec::with_capacity(4096);
    for offset in (0..4096).step_by(512) {
        engine.cpu_read(0x1000 + offset, 512, 1).unwrap();
        assert_eq!(
            &engine.arena()[HELPER_OFFSET + 16..HELPER_OFFSET + 20],
            &[0; 4]
        );
        value.extend_from_slice(&engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512]);
    }
    engine.arena_mut().unwrap().copy_from_slice(&saved);
    value
}

#[test]
fn wide_wrapping_guest_encodings_remain_outside_the_public_profile() {
    for (opcode, _) in FORMS {
        for w in [0, 0x80] {
            for operand in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe1, 0x6d | w, opcode, operand];
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd),
                    "{bytes:02x?}"
                );
            }
            let xmm = [0xc4, 0xe1, 0x69 | w, opcode, 0xcb];
            assert!(decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok());
        }
    }
}

#[test]
fn malformed_prefixes_are_rejected_before_the_public_capability_gate() {
    for (opcode, _) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe1, 0x6d, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn canonical_wide_helper_descriptors_refuse_without_any_arena_or_ram_publication() {
    let mut engine = EngineInstance::new(1, 0x57524150).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let before_ram = ram(&mut engine);
    for (_, kind) in FORMS {
        for operands in [1 | 2 << 3 | 3 << 6, 1 | 2 << 3 | 1 << 12] {
            // memory descriptors are canonical; stale address bytes cannot bypass
            // the helper's capability refusal or cause an operand access.
            engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(0x0100_0580 | kind, operands, 0, 0, 0, 2),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&mut engine), before_ram);
            assert_eq!(engine.arena(), before);
        }
    }
}
