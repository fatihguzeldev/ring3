use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation, ShiftKind},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError, ResidentInstallation},
};
use wasm_encoder::{
    EntityType, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType, Module,
    TypeSection, ValType,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x8abc_def0_f123_4567;
const KINDS: [(ShiftKind, u8); 3] = [
    (ShiftKind::Shl, 4),
    (ShiftKind::Shr, 5),
    (ShiftKind::Sar, 7),
];

#[derive(Clone, Copy, Debug)]
enum Owner {
    Replacement,
    Resident,
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 3, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, owner: Owner, entries: bool) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(1).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(1, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(1).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
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

fn forms() -> Vec<(Vec<u8>, Operation)> {
    let addresses: &[(&[u8], EffectiveAddress)] = &[
        (&[0x00], address(Some(Register32::Eax), None, 1, 0)),
        (&[0x04, 0x24], address(Some(Register32::Esp), None, 1, 0)),
        (&[0x45, 0], address(Some(Register32::Ebp), None, 1, 0)),
        (
            &[0x43, 0x80],
            address(Some(Register32::Ebx), None, 1, 0xffff_ff80),
        ),
        (
            &[0x83, 0x78, 0x56, 0x34, 0x92],
            address(Some(Register32::Ebx), None, 1, 0x9234_5678),
        ),
        (
            &[0x44, 0xcb, 0xfe],
            address(Some(Register32::Ebx), Some(Register32::Ecx), 8, 0xffff_fffe),
        ),
        (
            &[0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            address(None, Some(Register32::Ecx), 4, 0xffff_fff8),
        ),
        (
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
    ];
    let mut forms = Vec::new();
    for (kind, field) in KINDS {
        for &(tail, address) in addresses {
            let mut bytes = vec![0xd0, tail[0] | field << 3];
            bytes.extend_from_slice(&tail[1..]);
            forms.push((bytes, Operation::MemoryShiftByte { kind, address }));
        }
    }
    assert_eq!(forms.len(), 24);
    forms
}

fn declarations(owner: Owner) -> Vec<u8> {
    let resident = matches!(owner, Owner::Resident);
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function(
        std::iter::repeat_n(ValType::I32, if resident { 7 } else { 6 }),
        [ValType::I32],
    );
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function(
        std::iter::repeat_n(ValType::I32, if resident { 6 } else { 2 }),
        [ValType::I32],
    );
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import(
        "ring3",
        if resident { "guard_resident" } else { "guard" },
        EntityType::Function(1),
    );
    imports.import("ring3", "read8", EntityType::Function(2));
    imports.import(
        "ring3",
        if resident {
            "store_resident8"
        } else {
            "store8"
        },
        EntityType::Function(3),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 3);
    module.section(&exports);
    // this exact prefix covers declarations, not generated instruction bodies.
    module.finish()
}

#[test]
fn three_count_one_families_decode_eight_flat32_address_shapes() {
    let mut engine = code(CODE, &[0x90], false);
    for (bytes, expected) in forms() {
        upload(&mut engine, CODE, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(decoded.pc(), GuestAddress(CODE));
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
    }
}

#[test]
fn bound_profiles_require_narrow_declarations_while_standalone_refuses() {
    let mut bytes = Vec::new();
    let mut starts = Vec::new();
    for (instruction, _) in forms() {
        starts.push(CODE + bytes.len() as u32);
        bytes.extend(instruction);
    }
    starts.push(CODE + bytes.len() as u32);
    bytes.extend([0xeb, 0]);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(CODE), bytes.len())
                .unwrap();
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let pointer = engine.arena_address();
            let id = compile(&mut engine, owner, entries).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.arena_address(), pointer);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(u32::MAX), Access::Read)
                    .is_err()
            );
            let mut fetched = vec![0; bytes.len()];
            engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(CODE), &mut fetched)
                .unwrap();
            assert_eq!(fetched, bytes);
            guard(&engine, owner, id).unwrap();
            let artifact = match owner {
                Owner::Replacement => engine.artifact_bytes().unwrap(),
                Owner::Resident => {
                    for &pc in &starts {
                        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                    }
                    assert!(engine.lookup_resident(CODE + 1).is_err());
                    engine.resident_bytes(id).unwrap()
                }
            };
            assert!(artifact.starts_with(&declarations(owner)));
            assert!(artifact.len() <= CompileLimits::default().wasm_bytes);
        }
    }
    for (_, field) in KINDS {
        let bytes = [0xd0, field << 3 | 3, 0xeb, 0];
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: 4,
        }];
        let error = instruction_error(CODE, InstructionError::BackendUnsupported);
        for observed in [
            prepare_region(memory, &specs, CompileLimits::default()).err(),
            prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            compile_region(memory, &specs, CompileLimits::default()).err(),
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        ] {
            assert_eq!(observed, Some(error));
        }
    }
}

fn excluded() -> Vec<(Vec<u8>, DecodeError)> {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = Vec::new();
    for (_, field) in KINDS {
        cases.push((vec![0xc0, field << 3 | 3, 2], unsupported));
        cases.push((vec![0xd2, field << 3 | 3], unsupported));
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            cases.push((vec![prefix, 0xd0, field << 3 | 3], unsupported));
        }
        cases.push((
            vec![0xf0, 0xd0, field << 3 | 3],
            DecodeError::InvalidEncoding,
        ));
        cases.push((
            vec![0x64, 0xd0, field << 3 | 3],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ));
    }
    for field in [0, 1, 2, 3, 6] {
        let mut bytes = vec![0xd0, field << 3 | 3];
        if matches!(field, 0..=3) {
            bytes.insert(0, 0x66);
        }
        cases.push((bytes, unsupported));
    }
    assert_eq!(cases.len(), 29);
    cases
}

#[test]
fn count_prefix_segment_lock_rotate_and_sal_neighbors_keep_exact_decode_refusals() {
    for (bytes, expected) in excluded() {
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        assert_eq!(
            decode_one(memory, GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let error = instruction_error(CODE, InstructionError::Decode(expected));
        assert_eq!(
            compile_region(memory, &specs, CompileLimits::default()).err(),
            Some(error)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(error)
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 3],
    mapped: u32,
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(id).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        mapped: engine.memory().unwrap().mapped_pages(),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

#[test]
fn invalid_second_memory_instruction_preserves_both_published_owners_and_installation() {
    for (_, field) in KINDS {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(
                    CODE,
                    &[0xd0, field << 3 | 3, 0xc0, field << 3 | 3, 2],
                    false,
                );
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                engine.map(DATA, 1, 3).unwrap();
                engine.write32(DATA, 0x9234_5678).unwrap();
                describe(&mut engine, KEEP, 3, false);
                engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                engine
                    .acknowledge_resident_installation(KEY, keep, 3)
                    .unwrap();
                describe(&mut engine, CODE, 5, entries);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), 4096)
                    .unwrap();
                let before = publication(&engine, keep);
                let error = instruction_error(
                    CODE + 2,
                    InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                );
                let expected = match owner {
                    Owner::Replacement => HostError::Compile(error),
                    Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
                };
                assert_eq!(compile(&mut engine, owner, entries), Err(expected));
                assert_eq!(publication(&engine, keep), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}

#[test]
fn exact_instruction_fetch_stops_at_page_end_and_reports_each_missing_byte_and_wrap() {
    for (bytes, _) in forms() {
        let length = bytes.len() as u32;
        for pc in [0x2000 - length, u32::MAX - (length - 1)] {
            let engine = code(pc, &bytes, true);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length() as u32, length);
            assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(length)));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
        }
        for cut in 1..bytes.len() {
            for (pc, address, reason) in [
                (0x2000 - cut as u32, 0x2000, FaultReason::Unmapped),
                (
                    u32::MAX - (cut as u32 - 1),
                    u32::MAX - (cut as u32 - 1),
                    FaultReason::AddressOverflow,
                ),
            ] {
                let engine = code(pc, &bytes[..cut], true);
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(DecodeError::MemoryFault {
                        pc: GuestAddress(pc),
                        fault: MemoryFault {
                            address: GuestAddress(address),
                            access: Access::Execute,
                            reason
                        },
                        length: cut as u32 + 1,
                    })
                );
            }
        }
    }
}
