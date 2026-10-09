use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

#[test]
fn memory_conditional_move_admits_before_jump() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x0f, 0x44, 0x03, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(0x1000, code.len() as u32).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(code.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory conditional move must admit before a jump");
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Condition, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress},
};

const CODE: u32 = 0x1000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
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
const CONDITIONS: [Condition; 16] = [
    Condition::Overflow,
    Condition::NotOverflow,
    Condition::Below,
    Condition::AboveOrEqual,
    Condition::Equal,
    Condition::NotEqual,
    Condition::BelowOrEqual,
    Condition::Above,
    Condition::Sign,
    Condition::NotSign,
    Condition::Parity,
    Condition::NotParity,
    Condition::Less,
    Condition::GreaterOrEqual,
    Condition::LessOrEqual,
    Condition::Greater,
];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn address(
    base: Option<Register32>,
    index: Option<Register32>,
    scale: u8,
    displacement: u32,
) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    }
}

#[test]
fn every_condition_destination_and_address_form_has_exact_memory_ir() {
    let cases: [(&[u8], EffectiveAddress); 8] = [
        (&[0x00], address(Some(Register32::Eax), None, 1, 0)),
        (
            &[0x41, 0x80],
            address(Some(Register32::Ecx), None, 1, 0xffff_ff80),
        ),
        (
            &[0x44, 0x54, 0x7f],
            address(Some(Register32::Esp), Some(Register32::Edx), 2, 127),
        ),
        (
            &[0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff],
            address(Some(Register32::Ebx), Some(Register32::Esi), 8, 0xffff_ffe0),
        ),
        (&[0x45, 0], address(Some(Register32::Ebp), None, 1, 0)),
        (
            &[0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            address(None, Some(Register32::Ecx), 4, 0xffff_fff8),
        ),
        (
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
        (
            &[0x84, 0x94, 0x20, 0, 0, 0],
            address(Some(Register32::Esp), Some(Register32::Edx), 4, 32),
        ),
    ];
    let mut engine = code(CODE, &[0x90]);
    let mut decoded_forms = 0;
    for (cc, condition) in CONDITIONS.into_iter().enumerate() {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (tail, address) in cases {
                let mut bytes = vec![
                    0x0f,
                    0x40 + cc as u8,
                    tail[0] | (destination_index as u8) << 3,
                ];
                bytes.extend_from_slice(&tail[1..]);
                upload(&mut engine, CODE, &bytes);
                let before = engine.arena().to_vec();
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::ReadConditionalMove {
                        condition,
                        destination,
                        address
                    },
                    "{bytes:02x?}"
                );
                assert_eq!(
                    (usize::from(decoded.length()), decoded.next_pc()),
                    (bytes.len(), GuestAddress(CODE + bytes.len() as u32))
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                assert_eq!(engine.arena(), before);
                decoded_forms += 1;
            }
        }
    }
    assert_eq!(decoded_forms, 1024);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(DATA), Access::Read)
            .is_err()
    );
}

#[test]
fn bound_profiles_admit_unmapped_sources_while_standalone_fails_closed() {
    let mut bytes = Vec::new();
    for cc in 0..16 {
        bytes.extend_from_slice(&[0x0f, 0x40 + cc, ((cc % 8) << 3) | 3]);
    }
    bytes.extend_from_slice(&[0xeb, 0]);
    let engine = code(CODE, &bytes);
    let expected = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32
            }],
            CompileLimits::default()
        )
        .err(),
        Some(expected)
    );
    assert_eq!(
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(CODE)],
            CompileLimits::default()
        )
        .err(),
        Some(expected)
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes);
            engine.protect(CODE, 1, 4).unwrap();
            let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            request[..4].copy_from_slice(&CODE.to_le_bytes());
            request[4..]
                .copy_from_slice(&if entries { 0 } else { bytes.len() as u32 }.to_le_bytes());
            let before = engine.arena().to_vec();
            if resident {
                let id = if entries {
                    engine.compile_resident_entries(1, 0)
                } else {
                    engine.compile_resident(1)
                }
                .unwrap()
                .get();
                engine.guard_resident(KEY, id).unwrap();
                for offset in (0..48).step_by(3) {
                    assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                }
            } else {
                let generation = if entries {
                    engine.compile_entries(1, 0)
                } else {
                    engine.compile(1)
                }
                .unwrap();
                engine.guard(KEY, generation).unwrap();
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
        }
    }
}

#[test]
fn prefixes_and_exact_instruction_fetch_keep_fault_boundaries() {
    for prefix in [
        0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
    ] {
        let mut bytes = vec![prefix, 0x0f, 0x44, 0x03];
        if prefix == 0x66 {
            bytes.insert(0, 0x66);
        }
        let engine = code(CODE, &bytes);
        let expected = match prefix {
            0xf0 => DecodeError::InvalidEncoding,
            0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                DecodeError::Unsupported(UnsupportedFeature::Segment)
            }
            _ => DecodeError::Unsupported(UnsupportedFeature::Opcode),
        };
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected)
        );
    }
    let bytes = [0x0f, 0x44, 0x05, 0, 0x50, 0, 0];
    for pc in [0x1ff9, 0x1ffc, u32::MAX - 6] {
        let mut engine = code(pc, &bytes);
        let base = pc & !0xfff;
        let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
        engine.protect(base, pages, 4).unwrap();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (7, GuestAddress(pc.wrapping_add(7)))
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    let engine = code(0x1ffe, &[0x0f, 0x44]);
    assert!(
        matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1ffe)), Err(DecodeError::MemoryFault { fault, length: 3, .. }) if fault.reason == FaultReason::Unmapped && fault.address == GuestAddress(0x2000))
    );
    let mut engine = code(0x1ffc, &bytes);
    engine.protect(0x2000, 1, 3).unwrap();
    assert!(
        matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1ffc)), Err(DecodeError::MemoryFault { fault, length: 5, .. }) if fault.reason == FaultReason::Permission && fault.address == GuestAddress(0x2000))
    );
}

#[test]
fn data_only_mapping_and_writes_keep_code_and_owners_current() {
    let bytes = [0x0f, 0x44, 0x05, 0, 0x50, 0, 0, 0xeb, 0];
    let mut engine = code(CODE, &bytes);
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&CODE.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    engine.map(DATA, 1, 3).unwrap();
    engine.write32(DATA, 0x9234_5678).unwrap();
    engine.protect(DATA, 1, 1).unwrap();
    engine.guard(KEY, generation).unwrap();
    engine.guard_resident(KEY, id).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    engine.unmap(DATA, 1).unwrap();
    engine.guard(KEY, generation).unwrap();
    engine.guard_resident(KEY, id).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    engine
        .write32(CODE, u32::from_le_bytes([0x0f, 0x44, 0x05, 0]))
        .unwrap();
    assert!(
        !engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
    assert!(engine.guard(KEY, generation).is_err());
    assert!(engine.guard_resident(KEY, id).is_err());
}
