use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation, WordValue},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::EngineInstance,
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x574f_5244_4d45_4d31;

fn admits_without_data_access(bytes: &[u8]) {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    for access in [Access::Read, Access::Write] {
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0), access)
                .is_err()
        );
    }
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&CODE.to_le_bytes());
    descriptor[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compile(1),
        Ok(1),
        "bound WORD memory MOV must compile without accessing guest data"
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert!(!engine.artifact_bytes().unwrap().is_empty());
}

#[test]
fn bound_word_load_admits_without_data_access() {
    admits_without_data_access(&[0x66, 0x8b, 0x03, 0xeb, 0]);
}

#[test]
fn bound_word_register_store_admits_without_data_access() {
    admits_without_data_access(&[0x66, 0x89, 0x03, 0xeb, 0]);
}

#[test]
fn bound_word_immediate_store_admits_without_data_access() {
    admits_without_data_access(&[0x66, 0xc7, 0x03, 0x34, 0x12, 0xeb, 0]);
}

const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn code_space(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

fn instruction(opcode: u8, register: u8, tail: &[u8], immediate: Option<u16>) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, tail[0] | register << 3];
    bytes.extend_from_slice(&tail[1..]);
    if let Some(value) = immediate {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

fn addresses() -> [(&'static [u8], EffectiveAddress); 17] {
    use Register32::{Eax, Ebp, Ebx, Ecx, Esi, Esp};
    let address = |base, index, scale, displacement| EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    };
    [
        (&[0x00], address(Some(Eax), None, 1, 0)),
        (&[0x03], address(Some(Ebx), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Esp), None, 1, 0)),
        (&[0x45, 0], address(Some(Ebp), None, 1, 0)),
        (&[0x43, 0], address(Some(Ebx), None, 1, 0)),
        (&[0x43, 0x80], address(Some(Ebx), None, 1, 0xffff_ff80)),
        (&[0x43, 0x7f], address(Some(Ebx), None, 1, 127)),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            address(Some(Ebx), None, 1, 0x9234_5678),
        ),
        (&[0x04, 0x0b], address(Some(Ebx), Some(Ecx), 1, 0)),
        (&[0x04, 0x4b], address(Some(Ebx), Some(Ecx), 2, 0)),
        (&[0x04, 0x8b], address(Some(Ebx), Some(Ecx), 4, 0)),
        (&[0x04, 0xcb], address(Some(Ebx), Some(Ecx), 8, 0)),
        (
            &[0x04, 0x8d, 0x78, 0x56, 0x34, 0x92],
            address(None, Some(Ecx), 4, 0x9234_5678),
        ),
        (
            &[0x05, 0x78, 0x56, 0x34, 0x92],
            address(None, None, 1, 0x9234_5678),
        ),
        (
            &[0x44, 0xc0, 0xff],
            address(Some(Eax), Some(Eax), 8, u32::MAX),
        ),
        (&[0x44, 0xec, 0x7f], address(Some(Esp), Some(Ebp), 8, 127)),
        (
            &[0x84, 0xf4, 0, 0, 0, 0x80],
            address(Some(Esp), Some(Esi), 8, 0x8000_0000),
        ),
    ]
}

#[test]
fn all_word_aliases_and_complete_address_classes_have_exact_typed_ir() {
    let mut memory = code_space(CODE, &[0x90]);
    let mut checked = 0;
    for (tail, address) in addresses() {
        for (alias, register) in REGISTERS.into_iter().enumerate() {
            for (opcode, operation) in [
                (
                    0x8b,
                    Operation::LoadWord {
                        destination: register,
                        address,
                    },
                ),
                (
                    0x89,
                    Operation::StoreWord {
                        address,
                        source: WordValue::Register(register),
                    },
                ),
            ] {
                let bytes = instruction(opcode, alias as u8, tail, None);
                memory.write(GuestAddress(CODE), &bytes).unwrap();
                let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
                assert_eq!(decoded.operation(), &operation, "{bytes:02x?}");
                assert_eq!(decoded.length() as usize, bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
                checked += 1;
            }
        }
        for value in [0, 0x7fff, 0x8000, 0xffff, 0x6667, 0xf0f3] {
            let bytes = instruction(0xc7, 0, tail, Some(value));
            memory.write(GuestAddress(CODE), &bytes).unwrap();
            let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::StoreWord {
                    address,
                    source: WordValue::Immediate(value),
                },
                "{bytes:02x?}"
            );
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
            checked += 1;
        }
    }
    assert_eq!(checked, 374);
}

#[test]
fn prefix_like_displacement_and_immediate_bytes_are_payload() {
    let address = EffectiveAddress {
        base: Some(Register32::Esp),
        index: Some(Register32::Esi),
        scale: 8,
        displacement: 0xf3f0_6766,
    };
    let tail = [0x84, 0xf4, 0x66, 0x67, 0xf0, 0xf3];
    let mut memory = code_space(CODE, &[0x90]);
    for (bytes, expected) in [
        (
            instruction(0x8b, 4, &tail, None),
            Operation::LoadWord {
                destination: Register32::Esp,
                address,
            },
        ),
        (
            instruction(0x89, 4, &tail, None),
            Operation::StoreWord {
                address,
                source: WordValue::Register(Register32::Esp),
            },
        ),
        (
            instruction(0xc7, 0, &tail, Some(0x6766)),
            Operation::StoreWord {
                address,
                source: WordValue::Immediate(0x6766),
            },
        ),
    ] {
        memory.write(GuestAddress(CODE), &bytes).unwrap();
        let decoded = decode_one(&memory, GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.operation(), &expected);
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
    }
}

#[test]
fn complete_prefix_and_adjacent_forms_preserve_precise_refusal_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut memory = code_space(CODE, &[0x90]);
    let mut rejected = |bytes: &[u8], expected| {
        memory.write(GuestAddress(CODE), bytes).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    };
    for bytes in [
        instruction(0x8b, 0, &[0x03], None),
        instruction(0x89, 0, &[0x03], None),
        instruction(0xc7, 0, &[0x03], Some(0x1234)),
    ] {
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            rejected(&[vec![prefix], bytes.clone()].concat(), opcode);
        }
        rejected(&[vec![0x66, 0x67], bytes[1..].to_vec()].concat(), opcode);
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            rejected(&[vec![prefix], bytes.clone()].concat(), segment);
        }
        rejected(&[vec![0xf0], bytes].concat(), DecodeError::InvalidEncoding);
    }
    for bytes in [
        &[0x66, 0xa1, 0x10, 0x50, 0, 0][..],
        &[0x66, 0xa3, 0x10, 0x50, 0, 0],
        &[0x66, 0x8a, 0x03],
        &[0x66, 0x88, 0x03],
        &[0x66, 0xc6, 0x03, 0xff],
        &[0x66, 0x66, 0x03, 0x03],
        &[0x66, 0x66, 0x01, 0x03],
        &[0x66, 0x66, 0x81, 0x03, 0x34, 0x12],
    ] {
        rejected(bytes, opcode);
    }
    for bytes in [&[0x66, 0x8c, 0x03][..], &[0x66, 0x8e, 0x03]] {
        rejected(bytes, segment);
    }
    for extension in 1..8 {
        rejected(
            &instruction(0xc7, extension, &[0x03], Some(0x1234)),
            DecodeError::InvalidEncoding,
        );
    }
}

fn long_forms() -> [Vec<u8>; 3] {
    let tail = [0x84, 0xf4, 0x78, 0x56, 0x34, 0x92];
    [
        instruction(0x8b, 4, &tail, None),
        instruction(0x89, 4, &tail, None),
        instruction(0xc7, 0, &tail, Some(0xf0f3)),
    ]
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason, length: usize) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length: length as u32,
    }
}

#[test]
fn exact_instruction_fetch_crossings_truncations_and_top_span_faults() {
    for bytes in long_forms() {
        for pc in [
            0x2000 - bytes.len() as u32,
            0x1fff,
            u32::MAX - bytes.len() as u32 + 1,
        ] {
            let memory = code_space(pc, &bytes);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(
                decoded.next_pc(),
                GuestAddress(pc.wrapping_add(bytes.len() as u32))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            let memory = code_space(pc, &bytes[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, FaultReason::Unmapped, cut + 1))
            );
            let top = u32::MAX - cut as u32 + 1;
            let memory = code_space(top, &bytes[..cut]);
            assert_eq!(
                decode_one(&memory, GuestAddress(top)).err(),
                Some(fetch_error(top, top, FaultReason::AddressOverflow, cut + 1))
            );
        }
        let mut memory = code_space(CODE, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(CODE), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(CODE)).err(),
            Some(fetch_error(CODE, CODE, FaultReason::Permission, 1))
        );
        let mut memory = code_space(0x1fff, &bytes);
        memory
            .protect(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1fff)).err(),
            Some(fetch_error(0x1fff, 0x2000, FaultReason::Permission, 2))
        );
    }
}

#[test]
fn every_consumed_byte_tracks_same_and_changed_writes_without_data_dependencies() {
    for bytes in long_forms() {
        for (offset, original) in bytes.iter().copied().enumerate() {
            for replacement in [original, original ^ 1] {
                let mut memory = code_space(0x1ffc, &bytes);
                let decoded = decode_one(&memory, GuestAddress(0x1ffc)).unwrap();
                memory
                    .write(GuestAddress(0x1ffc + offset as u32), &[replacement])
                    .unwrap();
                assert!(!memory.is_code_current(decoded.code_snapshot()));
            }
        }
        let mut memory = code_space(0x1ffc, &bytes);
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x5000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1ffc)).unwrap();
        memory.write(GuestAddress(0x5000), &[0x66, 0x67]).unwrap();
        assert!(memory.is_code_current(decoded.code_snapshot()));
        memory
            .protect(
                PageRange::new(GuestAddress(0x5000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}
