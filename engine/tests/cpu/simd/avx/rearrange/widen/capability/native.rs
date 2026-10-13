use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const FORMS: [(u8, u32); 12] = [
    (0x20, 16),
    (0x21, 17),
    (0x22, 18),
    (0x23, 19),
    (0x24, 20),
    (0x25, 21),
    (0x30, 23),
    (0x31, 24),
    (0x32, 25),
    (0x33, 26),
    (0x34, 27),
    (0x35, 28),
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

fn admitted(bytes: &[u8]) -> ring3_engine::cpu::x86::ir::Operation {
    let memory = code(bytes);
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(
        matches!(
            instruction.operation(),
            ring3_engine::cpu::x86::ir::Operation::Avx(_)
        ),
        "{bytes:02x?}"
    );
    assert_eq!(instruction.pc(), GuestAddress(0x1000));
    assert_eq!(instruction.length() as usize, bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
    *instruction.operation()
}

fn zero_register_result(before: &[u8]) -> Vec<u8> {
    let mut expected = before.to_vec();
    let mut packet = [0; 40];
    packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet);
    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520].copy_from_slice(&2_u32.to_le_bytes());
    expected
}

#[test]
fn ymm_widening_encodings_bind_exact_memory_and_register_forms() {
    for (opcode, _) in FORMS {
        for w in [0, 0x80] {
            for operand in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe2, 0x7d | w, opcode, operand];
                admitted(&bytes);
            }
            let xmm = [0xc4, 0xe2, 0x79 | w, opcode, 0xcb];
            assert!(decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok());
        }
    }
}

#[test]
fn reserved_encoding_is_rejected_before_the_public_capability_gate() {
    for (opcode, _) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe2, 0x7d, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        for reserved in 0..15 {
            let bytes = [0xc4, 0xe2, 5 | reserved << 3, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn canonical_widen_descriptors_validate_state_metadata_and_register_publication() {
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
    for (_, kind) in FORMS {
        for operands in [1 | (3 << 6), 1 | (1 << 12)] {
            let descriptor = 0x01000680 | kind;
            let immediate = 0;
            let flags = 2;
            let valid_register = operands & (1 << 12) == 0;
            engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
            engine.arena_mut().unwrap()[YMM_OFFSET + 12] = 1;
            let malformed_state = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(descriptor, operands, immediate, 0, 0, flags),
                Err(HostError::Infrastructure)
            );
            assert_eq!(engine.arena(), malformed_state);

            let mut after_ram = [0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x1000), &mut after_ram)
                .unwrap();
            assert_eq!(&after_ram[..], &before_ram[..]);
            assert_eq!(engine.arena(), malformed_state);
            engine.arena_mut().unwrap()[YMM_OFFSET + 12] = 0;
            let before = engine.arena().to_vec();
            let result = engine.avx_execute(descriptor, operands, immediate, 0, 0, flags);
            if valid_register {
                assert_eq!(result, Ok(StoreCompletion::Complete));
                assert_eq!(engine.arena(), zero_register_result(&before));
            } else {
                assert_eq!(result, Err(HostError::InvalidRequest));
                assert_eq!(engine.arena(), before);
            }

            let mut after_ram = [0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x1000), &mut after_ram)
                .unwrap();
            assert_eq!(&after_ram[..], &before_ram[..]);
            if valid_register {
                assert_eq!(engine.arena(), zero_register_result(&before));
            } else {
                assert_eq!(engine.arena(), before);
            }
        }
    }
}
