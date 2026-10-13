use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, YMM_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
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

#[test]
fn lane_local_ymm_encodings_remain_private_and_all_xmm_forms_stay_available() {
    for form in FORMS {
        for w in [false, true] {
            if w && form.kind == 31 {
                continue;
            }
            for memory in [false, true] {
                let bytes = encoding(form, true, w, [0, 1, 2, 3], form.control(), memory);
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd)
                );
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
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        if form.unary() {
            for reserved in 0..15 {
                let mut bytes = valid.clone();
                bytes[2] = bytes[2] & !0x78 | reserved << 3;
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
        }
        if form.kind == 31 {
            let mut bytes = valid.clone();
            bytes[2] |= 0x80;
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn canonical_lane_ymm_helper_descriptors_refuse_before_metadata_or_state() {
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
            let arena = engine.arena_mut().unwrap();
            arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
            arena[YMM_OFFSET + 12] = 1;
            let before = arena.to_vec();
            assert_eq!(
                engine.avx_execute(
                    0x01000680 | u32::from(form.kind),
                    operands,
                    if form.immediate() {
                        u32::from(form.control())
                    } else {
                        0
                    },
                    0,
                    0,
                    2
                ),
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
        }
    }
}
