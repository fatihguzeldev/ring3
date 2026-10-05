use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Location32, Operation, UnaryKind},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;

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

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn instruction_error(pc: u32, error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(error),
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length,
    }
}

#[test]
fn exact_cdq_ir_and_fixed32_adjacency_compile_in_all_six_profiles() {
    let adjacent = code(CODE, &[0x48, 0x99]);
    let first = decode_one(adjacent.memory().unwrap(), GuestAddress(CODE)).unwrap();
    assert_eq!(
        first.operation(),
        &Operation::Unary {
            kind: UnaryKind::Dec,
            destination: Location32::Register(Register32::Eax)
        }
    );
    assert_eq!(
        (first.length(), first.next_pc()),
        (1, GuestAddress(CODE + 1))
    );
    let second = decode_one(adjacent.memory().unwrap(), first.next_pc()).unwrap();
    assert_eq!(second.operation(), &Operation::SignExtendHigh);
    assert_eq!(
        (second.length(), second.next_pc()),
        (1, GuestAddress(CODE + 2))
    );
    assert!(
        adjacent
            .memory()
            .unwrap()
            .is_code_current(second.code_snapshot())
    );

    let bytes = [0x99, 0xeb, 0];
    let engine = code(CODE, &bytes);
    let memory = engine.memory().unwrap();
    for unit in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 3,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (unit.metadata().blocks, unit.metadata().instructions),
            (1, 2)
        );
        assert_eq!(&unit.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes);
            engine.protect(CODE, 1, 4).unwrap();
            describe(&mut engine, CODE, 3, entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, resident, entries).unwrap();
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                for pc in [CODE, CODE + 1] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 2).is_err());
            } else {
                engine.guard(KEY, id as u32).unwrap();
                assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
            }
            assert_eq!(engine.arena(), before);
        }
    }
}

#[test]
fn strict_prefixes_and_single_byte_fetch_preserve_fault_and_wrap_boundaries() {
    for prefix in [
        0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
    ] {
        let engine = code(CODE, &[prefix, 0x99]);
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
    for pc in [0x1fff, u32::MAX] {
        let mut engine = code(pc, &[0x99]);
        engine.protect(pc & !0xfff, 1, 4).unwrap();
        let memory = engine.memory().unwrap();
        let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::SignExtendHigh);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (1, GuestAddress(pc.wrapping_add(1)))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
        assert_eq!(
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(pc),
                    byte_length: 1
                }],
                CompileLimits::default()
            )
            .unwrap()
            .metadata()
            .instructions,
            1
        );
        if pc == 0x1fff {
            assert!(
                memory
                    .resolve(GuestAddress(0x2000), Access::Execute)
                    .is_err()
            );
        }
    }
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Unmapped),
        (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
    ] {
        let engine = code(pc, &[0x66]);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(fetch_error(pc, address, 2, reason))
        );
    }
    let mut engine = code(CODE, &[0x99]);
    engine.protect(CODE, 1, 3).unwrap();
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
        Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
    );
}

#[test]
fn late_cdq_failures_preserve_replacement_and_resident_publications() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refusals = 0;
    for (pc, bytes, declared, expected) in [
        (
            CODE,
            &[0x99, 0x0f, 0x0b][..],
            3,
            instruction_error(CODE + 1, opcode),
        ),
        (
            CODE,
            &[0x99, 0x66, 0x99][..],
            3,
            instruction_error(CODE + 1, opcode),
        ),
        (
            0x1ffe,
            &[0x99, 0x0f][..],
            3,
            instruction_error(
                0x1fff,
                fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
            ),
        ),
    ] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(pc, bytes);
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                describe(&mut engine, KEEP, 3, false);
                let generation = engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                describe(&mut engine, pc, declared, entries);
                let before = (
                    engine.arena().to_vec(),
                    engine.generation(),
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(keep).unwrap().to_vec(),
                );
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(expected))
                } else {
                    HostError::Compile(expected)
                };
                assert_eq!(compile(&mut engine, resident, entries), Err(expected));
                assert_eq!(
                    (
                        engine.arena().to_vec(),
                        engine.generation(),
                        engine.artifact_bytes().unwrap().to_vec(),
                        engine.resident_bytes(keep).unwrap().to_vec()
                    ),
                    before
                );
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(pc).is_err());
                refusals += 1;
            }
        }
    }
    assert_eq!(refusals, 12);
}
