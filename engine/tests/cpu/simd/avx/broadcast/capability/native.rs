use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const FORMS: &[(u8, u8, bool)] = &[
    (0x78, 0x79, true),
    (0x78, 0x7d, true),
    (0x79, 0x79, true),
    (0x79, 0x7d, true),
    (0x58, 0x79, true),
    (0x58, 0x7d, true),
    (0x59, 0x79, true),
    (0x59, 0x7d, true),
    (0x18, 0x79, false),
    (0x18, 0x7d, false),
    (0x19, 0x7d, false),
];

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn decode(bytes: &[u8]) -> Result<(), DecodeError> {
    let memory = memory(bytes);
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
    let memory = memory(bytes);
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
fn new_broadcast_forms_bind_exact_operands_and_lengths() {
    assert_eq!(
        FORMS
            .iter()
            .map(|(_, _, memory)| 1 + usize::from(*memory))
            .sum::<usize>(),
        19
    );
    for &(opcode, control, memory) in FORMS {
        for destination in 0..8 {
            for source in 0..8 {
                let bytes = [
                    0xc4,
                    0xe2,
                    control,
                    opcode,
                    0xc0 | destination << 3 | source,
                ];
                admitted(&bytes);
            }
            if memory {
                let bytes = [0xc4, 0xe2, control, opcode, destination << 3];
                admitted(&bytes);
            }
        }
    }
}

#[test]
fn reserved_w_vvvv_prefixes_and_mode_neighbors_fail_before_avx2_gate() {
    for &(opcode, control, memory) in FORMS {
        for modrm in [Some(0xcb), memory.then_some(0x08)].into_iter().flatten() {
            for changed in [control | 0x80, control & !8] {
                assert_eq!(
                    decode(&[0xc4, 0xe2, changed, opcode, modrm]),
                    Err(DecodeError::InvalidEncoding)
                );
            }
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                assert_eq!(
                    decode(&[prefix, 0xc4, 0xe2, control, opcode, modrm]),
                    Err(DecodeError::InvalidEncoding)
                );
            }
        }
    }
    for opcode in [0x19, 0x5a] {
        for modrm in [0x08, 0xcb] {
            assert_eq!(
                decode(&[0xc4, 0xe2, 0x79, opcode, modrm]),
                Err(DecodeError::InvalidEncoding)
            );
        }
    }
}

#[test]
fn scalar_and_block_raw_aliases_share_canonical_bindings() {
    for (control, opcode) in [(0x79, 0x18), (0x7d, 0x18), (0x7d, 0x19)] {
        assert_eq!(decode(&[0xc4, 0xe2, control, opcode, 0x08]), Ok(()));
    }
    for (integer, float) in [
        (
            &[0xc4, 0xe2, 0x7d, 0x5a, 0x08][..],
            &[0xc4, 0xe2, 0x7d, 0x1a, 0x08][..],
        ),
        (
            &[0xc4, 0xe3, 0x6d, 0x38, 0xcb, 1][..],
            &[0xc4, 0xe3, 0x6d, 0x18, 0xcb, 1][..],
        ),
        (
            &[0xc4, 0xe3, 0x7d, 0x39, 0xcb, 1][..],
            &[0xc4, 0xe3, 0x7d, 0x19, 0xcb, 1][..],
        ),
        (
            &[0xc4, 0xe3, 0x6d, 0x46, 0xcb, 0x21][..],
            &[0xc4, 0xe3, 0x6d, 0x06, 0xcb, 0x21][..],
        ),
    ] {
        assert_eq!(admitted(integer), admitted(float));
        assert_eq!(decode(float), Ok(()));
    }
}

#[test]
fn broadcast_register_success_and_malformed_requests_keep_exact_publication() {
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
    for kind in 8..=15 {
        for (operands, flags) in [1 | (3 << 6), 1 | (1 << 12)]
            .into_iter()
            .flat_map(|operands| [2, 0xffff_ffff].map(|flags| (operands, flags)))
        {
            let descriptor = 0x01000e00 | kind;
            let immediate = 0;
            let valid_register = flags == 2 && operands & (1 << 12) == 0;
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
