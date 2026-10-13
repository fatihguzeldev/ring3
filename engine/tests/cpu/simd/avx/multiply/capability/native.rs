use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};
const FORMS: &[(u8, u8)] = &[
    (1, 0xd5),
    (1, 0xe4),
    (1, 0xe5),
    (1, 0xf4),
    (1, 0xf5),
    (1, 0xf6),
    (2, 1),
    (2, 2),
    (2, 3),
    (2, 4),
    (2, 5),
    (2, 6),
    (2, 7),
    (2, 0x0b),
    (2, 0x28),
    (2, 0x40),
];
fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn decode(bytes: &[u8]) -> Result<(), DecodeError> {
    let memory = code(bytes);
    let snapshot = memory
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    let result = decode_one(&memory, GuestAddress(0x1000)).map(|instruction| {
        assert_eq!(usize::from(instruction.length()), bytes.len());
        assert!(memory.is_code_current(instruction.code_snapshot()));
    });
    assert!(memory.is_code_current(&snapshot));
    result
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
fn wide_multiply_codes_bind_both_wig_values() {
    for &(map, opcode) in FORMS {
        for w in [0, 0x80] {
            for second in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe0 | map, 0x6d | w, opcode, second];
                admitted(&bytes);
                let mut xmm = bytes;
                xmm[2] &= !4;
                assert_eq!(decode(&xmm), Ok(()));
            }
        }
    }
}

#[test]
fn invalid_prefixes_and_unassigned_shapes_stay_precise() {
    for &(map, opcode) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            assert_eq!(
                decode(&[prefix, 0xc4, 0xe0 | map, 0x6d, opcode, 0xcb]),
                Err(DecodeError::InvalidEncoding)
            );
        }
    }
    assert_eq!(
        decode(&[0xc4, 0xe2, 0x7d, 0x41, 0xcb]),
        Err(DecodeError::InvalidEncoding)
    );
    admitted(&[0xc4, 0xe3, 0x6d, 0x42, 0xcb, 0x29]);
}

#[test]
fn wide_multiply_register_success_preserves_unsupported_shape_and_metadata_controls() {
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
    for kind in 1..=18 {
        for operands in [1 | (2 << 3) | (3 << 6), 1 | (2 << 3) | (1 << 12)] {
            let descriptor = 0x01000880 | kind;
            let immediate = 0;
            let flags = 2;
            let valid_register = kind != 17 && operands & (1 << 12) == 0;
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

#[test]
fn controlled_wide_sad_binds_while_xmm_controls_remain_available() {
    for w in [0, 0x80] {
        for second in [0xcb, 0x08] {
            for immediate in [0, 8, 200, 255] {
                let bytes = [0xc4, 0xe3, 0x6d | w, 0x42, second, immediate];
                admitted(&bytes);
                let mut xmm = bytes;
                xmm[2] &= !4;
                assert_eq!(decode(&xmm), Ok(()));
                for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                    let mut forbidden = vec![prefix];
                    forbidden.extend(bytes);
                    assert_eq!(decode(&forbidden), Err(DecodeError::InvalidEncoding));
                }
            }
        }
    }
}
