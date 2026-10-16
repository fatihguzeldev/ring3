use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const FORMS: &[(u8, u8, u32, bool)] = &[
    (1, 0xd8, 8, false),
    (1, 0xd9, 9, false),
    (1, 0xda, 10, false),
    (1, 0xdc, 11, false),
    (1, 0xdd, 12, false),
    (1, 0xde, 13, false),
    (1, 0xe0, 14, false),
    (1, 0xe3, 15, false),
    (1, 0xe8, 16, false),
    (1, 0xe9, 17, false),
    (1, 0xea, 18, false),
    (1, 0xec, 19, false),
    (1, 0xed, 20, false),
    (1, 0xee, 21, false),
    (2, 0x08, 29, false),
    (2, 0x09, 30, false),
    (2, 0x0a, 31, false),
    (2, 0x1c, 32, true),
    (2, 0x1d, 33, true),
    (2, 0x1e, 34, true),
    (2, 0x38, 37, false),
    (2, 0x39, 38, false),
    (2, 0x3a, 39, false),
    (2, 0x3b, 40, false),
    (2, 0x3c, 41, false),
    (2, 0x3d, 42, false),
    (2, 0x3e, 43, false),
    (2, 0x3f, 44, false),
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

fn encoding(map: u8, opcode: u8, unary: bool, wide: bool, w: bool, memory: bool) -> [u8; 5] {
    let vvvv = if unary { 15 } else { 13 };
    [
        0xc4,
        0xe0 | map,
        vvvv << 3 | if wide { 4 } else { 0 } | 1 | if w { 0x80 } else { 0 },
        opcode,
        if memory { 0x08 } else { 0xcb },
    ]
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
fn wide_integer_encodings_bind_both_wig_values_and_memory_forms() {
    for &(map, opcode, _, unary) in FORMS {
        for w in [false, true] {
            for memory in [false, true] {
                let bytes = encoding(map, opcode, unary, true, w, memory);
                admitted(&bytes);
                let xmm = encoding(map, opcode, unary, false, w, memory);
                assert!(
                    decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok(),
                    "{xmm:02x?}"
                );
            }
        }
    }
}

#[test]
fn malformed_prefixes_and_unary_vvvv_are_rejected_before_the_gate() {
    for &(map, opcode, _, unary) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(map, opcode, unary, true, false, false));
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
        if unary {
            let mut bytes = encoding(map, opcode, true, true, false, false);
            bytes[2] &= !8;
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}

#[test]
fn canonical_integer_register_success_and_bad_metadata_are_distinct() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let mut before_ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();
    for &(_, _, kind, unary) in FORMS {
        let first = if unary { 0 } else { 2 << 3 };
        for operands in [1 | first | (3 << 6), 1 | first | (1 << 12)] {
            let descriptor = 0x01000580 | kind;
            let immediate = 0;
            let flags = 2;
            let valid_register = operands & (1 << 12) == 0;
            engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
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
