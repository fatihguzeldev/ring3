use ring3_engine::{
    abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

#[derive(Clone, Copy)]
struct Form {
    opcode: u8,
    map: u8,
    pp: u8,
    kind: u8,
}
const FORMS: [Form; 19] = [
    Form {
        opcode: 0x60,
        map: 1,
        pp: 1,
        kind: 1,
    },
    Form {
        opcode: 0x61,
        map: 1,
        pp: 1,
        kind: 2,
    },
    Form {
        opcode: 0x62,
        map: 1,
        pp: 1,
        kind: 3,
    },
    Form {
        opcode: 0x63,
        map: 1,
        pp: 1,
        kind: 4,
    },
    Form {
        opcode: 0x67,
        map: 1,
        pp: 1,
        kind: 5,
    },
    Form {
        opcode: 0x68,
        map: 1,
        pp: 1,
        kind: 6,
    },
    Form {
        opcode: 0x69,
        map: 1,
        pp: 1,
        kind: 7,
    },
    Form {
        opcode: 0x6a,
        map: 1,
        pp: 1,
        kind: 8,
    },
    Form {
        opcode: 0x6b,
        map: 1,
        pp: 1,
        kind: 9,
    },
    Form {
        opcode: 0x6c,
        map: 1,
        pp: 1,
        kind: 10,
    },
    Form {
        opcode: 0x6d,
        map: 1,
        pp: 1,
        kind: 11,
    },
    Form {
        opcode: 0x70,
        map: 1,
        pp: 1,
        kind: 12,
    },
    Form {
        opcode: 0x70,
        map: 1,
        pp: 2,
        kind: 13,
    },
    Form {
        opcode: 0x70,
        map: 1,
        pp: 3,
        kind: 14,
    },
    Form {
        opcode: 0x00,
        map: 2,
        pp: 1,
        kind: 15,
    },
    Form {
        opcode: 0x2b,
        map: 2,
        pp: 1,
        kind: 22,
    },
    Form {
        opcode: 0x0e,
        map: 3,
        pp: 1,
        kind: 29,
    },
    Form {
        opcode: 0x0f,
        map: 3,
        pp: 1,
        kind: 30,
    },
    Form {
        opcode: 0x4c,
        map: 3,
        pp: 1,
        kind: 31,
    },
];

impl Form {
    fn unary(self) -> bool {
        matches!(self.kind, 12..=14)
    }
    fn immediate(self) -> bool {
        matches!(self.kind, 12..=14 | 29 | 30)
    }
    fn control(self) -> u8 {
        if self.kind == 29 {
            0x96
        } else if self.kind == 30 {
            19
        } else if self.immediate() {
            0x1b
        } else {
            0
        }
    }
}

fn encoding(form: Form, wide: bool, w: bool, roles: [u8; 4], control: u8, memory: bool) -> Vec<u8> {
    let [destination, first, second, mask] = roles;
    let vvvv = if form.unary() { 15 } else { 15 - first };
    let mut bytes = vec![
        0xc4,
        0xe0 | form.map,
        vvvv << 3 | if wide { 4 } else { 0 } | form.pp | if w { 0x80 } else { 0 },
        form.opcode,
        (if memory { 0 } else { 0xc0 }) | destination << 3 | if memory { 0 } else { second },
    ];
    if form.kind == 31 {
        bytes.push(mask << 4 | control);
    } else if form.immediate() {
        bytes.push(control);
    }
    bytes
}
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
fn lane_local_ymm_encodings_bind_while_xmm_forms_stay_available() {
    for form in FORMS {
        for w in [false, true] {
            if w && form.kind == 31 {
                continue;
            }
            for memory in [false, true] {
                let bytes = encoding(form, true, w, [0, 1, 2, 3], form.control(), memory);
                admitted(&bytes);
                let xmm = encoding(form, false, w, [0, 1, 2, 3], form.control(), memory);
                assert!(decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok());
            }
        }
    }
}

#[test]
fn malformed_encoding_is_rejected_before_the_lane_capability_gate() {
    for form in FORMS {
        let valid = encoding(form, true, false, [0, 1, 2, 3], form.control(), false);
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(&valid);
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
        if form.unary() {
            for reserved in 0..15 {
                let mut bytes = valid.clone();
                bytes[2] = bytes[2] & !0x78 | reserved << 3;
                crate::support_fetch::assert_decode_contract(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding,
                );
            }
        }
        if form.kind == 31 {
            let mut bytes = valid.clone();
            bytes[2] |= 0x80;
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}

#[test]
fn canonical_lane_descriptors_validate_state_metadata_and_register_publication() {
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
    for form in FORMS {
        for memory in [false, true] {
            let operands = 1
                | if form.unary() { 0 } else { 2 << 3 }
                | if memory { 1 << 12 } else { 3 << 6 }
                | if form.kind == 31 { 4 << 9 } else { 0 };
            let descriptor = 0x01000680 | u32::from(form.kind);
            let immediate = if form.immediate() {
                u32::from(form.control())
            } else {
                0
            };
            let flags = 2;
            let valid_register = !memory;
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
