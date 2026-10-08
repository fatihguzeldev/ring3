use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admission(modrm: u8) {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0xf7, modrm, 0xeb, 0]);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("memory DWORD division admits through the existing public compiler");
}

#[test]
fn memory_div_admits_public_api() {
    admission(0x33);
}

#[test]
fn memory_idiv_admits_public_api() {
    admission(0x3b);
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{DivideKind, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEY: u64 = 0x1234_5678_9abc_def0;

fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0_u32 } else { length }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn encoded(kind: DivideKind, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xf7];
    bytes.extend_from_slice(tail);
    bytes[1] |= if kind == DivideKind::Unsigned {
        0x30
    } else {
        0x38
    };
    bytes
}

fn forms() -> Vec<(Vec<u8>, EffectiveAddress)> {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esp};
    [
        (vec![0x03], Some(Ebx), None, 1, 0),
        (vec![0x00], Some(Eax), None, 1, 0),
        (vec![0x02], Some(Edx), None, 1, 0),
        (vec![0x04, 0x24], Some(Esp), None, 1, 0),
        (vec![0x44, 0x8f, 0x80], Some(Edi), Some(Ecx), 4, 0xffff_ff80),
        (vec![0x45, 0x80], Some(Ebp), None, 1, 0xffff_ff80),
        (vec![0x83, 0x10, 0, 0, 0], Some(Ebx), None, 1, 0x10),
        (vec![0x05, 0x10, 0x50, 0, 0], None, None, 1, 0x5010),
    ]
    .into_iter()
    .map(|(tail, base, index, scale, displacement)| {
        (
            tail,
            EffectiveAddress {
                base,
                index,
                scale,
                displacement,
            },
        )
    })
    .collect()
}

#[test]
fn memory_division_ea_identities_compile_in_four_bound_profiles() {
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for kind in [DivideKind::Unsigned, DivideKind::Signed] {
        for (tail, address) in forms() {
            let form = encoded(kind, &tail);
            expected.push((CODE + bytes.len() as u32, form.len(), kind, address));
            bytes.extend(form);
        }
    }
    bytes.extend([0xeb, 0]);
    let engine = fixture(CODE, &bytes);
    for (pc, length, kind, address) in expected {
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ReadDivideAccumulator { kind, address }
        );
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (length, GuestAddress(pc + length as u32))
        );
    }
    let memory = engine.memory().unwrap();
    let unsupported = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32
            }],
            CompileLimits::default()
        )
        .err(),
        Some(unsupported)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        Some(unsupported)
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(CODE, &bytes);
            describe(&mut engine, CODE, bytes.len() as u32, entries);
            let before = engine.arena().to_vec();
            compile(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
        }
    }
}

#[test]
fn strict_neighbors_and_operand_fetch_keep_categories_and_exact_faults() {
    for kind in [DivideKind::Unsigned, DivideKind::Signed] {
        for prefix in [0x66, 0x67, 0xf2, 0xf3, 0xf0, 0x26, 0x64] {
            let bytes = [vec![prefix], encoded(kind, &[0x03])].concat();
            let engine = fixture(CODE, &bytes);
            let wanted = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x64 => DecodeError::Unsupported(UnsupportedFeature::Segment),
                _ => DecodeError::Unsupported(UnsupportedFeature::Opcode),
            };
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(wanted)
            );
        }
        for tail in [
            &[0x03][..],
            &[0x05, 0x10, 0x50, 0, 0],
            &[0x84, 0x8f, 0x10, 0x50, 0, 0],
        ] {
            let bytes = encoded(kind, tail);
            for pc in [
                0x2000 - bytes.len() as u32,
                u32::MAX - (bytes.len() as u32 - 1),
                0x1fff,
            ] {
                let mut engine = fixture(pc, &bytes);
                engine
                    .protect(pc & !0xfff, if pc == 0x1fff { 2 } else { 1 }, 4)
                    .unwrap();
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (
                        bytes.len(),
                        GuestAddress(pc.wrapping_add(bytes.len() as u32))
                    )
                );
            }
            let missing = &bytes[..bytes.len() - 1];
            if tail.len() == 1 && kind == DivideKind::Signed {
                continue;
            }
            let pc = 0x2000 - missing.len() as u32;
            let engine = fixture(pc, missing);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped
                    },
                    length: bytes.len() as u32,
                })
            );
        }
    }
    for bytes in [&[0x66, 0xf6, 0x33][..], &[0x66, 0xf6, 0x3b]] {
        let engine = fixture(CODE, bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
}

#[test]
fn data_repairs_keep_code_current_and_late_refusals_preserve_publication() {
    for kind in [DivideKind::Unsigned, DivideKind::Signed] {
        let mut bytes = encoded(kind, &[0x05, 0x10, 0x50, 0, 0]);
        bytes.extend([0xeb, 0]);
        let mut engine = fixture(CODE, &bytes);
        describe(&mut engine, CODE, bytes.len() as u32, false);
        let generation = engine.compile(1).unwrap();
        let id = engine.compile_resident(1).unwrap().get();
        engine.map(0x5000, 1, 3).unwrap();
        for value in [0, 5, u32::MAX] {
            engine.write32(0x5010, value).unwrap();
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, id).unwrap();
        }
        engine.protect(0x5000, 1, 2).unwrap();
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, id).unwrap();
        engine.map(0x3000, 1, 7).unwrap();
        for resident in [false, true] {
            for entries in [false, true] {
                let mut bad = encoded(kind, &[0x03]);
                bad.extend([0x0f, 0x0b]);
                engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bad.len()]
                    .copy_from_slice(&bad);
                engine.upload(0x3000, bad.len() as u32).unwrap();
                describe(&mut engine, 0x3000, bad.len() as u32, entries);
                let before = (
                    engine.arena().to_vec(),
                    engine.generation(),
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(id).unwrap().to_vec(),
                );
                let failure = CompileError::Instruction {
                    pc: GuestAddress(0x3002),
                    cause: InstructionError::Decode(DecodeError::Unsupported(
                        UnsupportedFeature::Opcode,
                    )),
                };
                let wanted = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                assert_eq!(compile(&mut engine, resident, entries).err(), Some(wanted));
                assert_eq!(
                    (
                        engine.arena().to_vec(),
                        engine.generation(),
                        engine.artifact_bytes().unwrap().to_vec(),
                        engine.resident_bytes(id).unwrap().to_vec()
                    ),
                    before
                );
            }
        }
    }
}
