use ring3_engine::process::EngineInstance;

#[test]
fn memory_set_byte_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x0f, 0x94, 0x06, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
    engine.upload(0x1000, 5).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&5_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("memory SETcc must admit in the bound engine");
}
use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            RegistryLimits, compile_entry_region, compile_region, prepare_entry_region,
            prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Condition, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{HostError, ResidentInstallation},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x8abc_def0_f123_4567;
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
const ARENA_BYTES: usize = 4_236;

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
    assert_eq!(engine.arena().len(), ARENA_BYTES);
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

fn compile_count(
    engine: &mut EngineInstance,
    owner: Owner,
    entries: bool,
    count: u32,
) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(count).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(count, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(count).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    }
}

fn compile(engine: &mut EngineInstance, owner: Owner, entries: bool) -> Result<u64, HostError> {
    compile_count(engine, owner, entries, 1)
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
}

fn host_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn encoding(condition: u8, ignored_reg: u8, tail: &[u8]) -> Vec<u8> {
    assert!(condition < 16 && ignored_reg < 8);
    assert_eq!(tail[0] & 0x38, 0);
    assert_ne!(tail[0] & 0xc0, 0xc0);
    let mut bytes = vec![0x0f, 0x90 + condition, tail[0] | ignored_reg << 3];
    bytes.extend_from_slice(&tail[1..]);
    bytes
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

fn address_cases() -> Vec<(Vec<u8>, EffectiveAddress)> {
    vec![
        (vec![0x00], address(Some(Register32::Eax), None, 1, 0)),
        (
            vec![0x41, 0x80],
            address(Some(Register32::Ecx), None, 1, (-128_i32) as u32),
        ),
        (
            vec![0x44, 0x54, 0x7f],
            address(Some(Register32::Esp), Some(Register32::Edx), 2, 127),
        ),
        (
            vec![0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff],
            address(
                Some(Register32::Ebx),
                Some(Register32::Esi),
                8,
                (-32_i32) as u32,
            ),
        ),
        (vec![0x45, 0], address(Some(Register32::Ebp), None, 1, 0)),
        (
            vec![0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            address(None, Some(Register32::Ecx), 4, (-8_i32) as u32),
        ),
        (
            vec![0x05, 0xff, 0xff, 0xff, 0xff],
            address(None, None, 1, u32::MAX),
        ),
        (
            vec![0x84, 0x94, 0x20, 0, 0, 0],
            address(Some(Register32::Esp), Some(Register32::Edx), 4, 32),
        ),
    ]
}

fn canonical_forms() -> Vec<(Vec<u8>, Operation)> {
    let mut forms = Vec::new();
    for (index, condition) in CONDITIONS.into_iter().enumerate() {
        for ignored_reg in 0..8 {
            forms.push((
                encoding(index as u8, ignored_reg, &[0x06]),
                Operation::MemorySetByte {
                    condition,
                    address: address(Some(Register32::Esi), None, 1, 0),
                },
            ));
        }
    }
    assert_eq!(forms.len(), 128);
    forms
}

#[test]
fn all_conditions_ignored_fields_and_effective_addresses_have_exact_ir() {
    let mut engine = code(CODE, &[0x90], false);
    let mut forms = canonical_forms();
    let cases = address_cases();
    assert_eq!(cases.len(), 8);
    for (tail, address) in cases {
        for (index, condition) in CONDITIONS.into_iter().enumerate() {
            for ignored_reg in 0..8 {
                forms.push((
                    encoding(index as u8, ignored_reg, &tail),
                    Operation::MemorySetByte { condition, address },
                ));
            }
        }
    }
    assert_eq!(forms.len(), 1_152);
    for (bytes, expected) in forms {
        upload(&mut engine, CODE, &bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(DATA), Access::Read)
                .is_err()
        );
    }
}

fn unsigned(bytes: &[u8], cursor: &mut usize) -> usize {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let byte = bytes[*cursor];
        *cursor += 1;
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
    }
    panic!("generated unsigned value exceeds u32");
}

fn name(bytes: &[u8], cursor: &mut usize) -> String {
    let length = unsigned(bytes, cursor);
    let value = std::str::from_utf8(&bytes[*cursor..*cursor + length])
        .unwrap()
        .to_owned();
    *cursor += length;
    value
}

fn store_only_declarations(bytes: &[u8], owner: Owner) {
    assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
    let mut sections = [None; 13];
    let mut cursor = 8;
    while cursor < bytes.len() {
        let id = bytes[cursor] as usize;
        cursor += 1;
        let length = unsigned(bytes, &mut cursor);
        assert!(id != 0 && id < sections.len());
        assert!(sections[id].is_none());
        sections[id] = Some(&bytes[cursor..cursor + length]);
        cursor += length;
    }
    assert_eq!(cursor, bytes.len());
    let guard_parameters = match owner {
        Owner::Replacement => 6,
        Owner::Resident => 7,
    };
    let store_parameters = match owner {
        Owner::Replacement => 2,
        Owner::Resident => 6,
    };
    let section = sections[1].unwrap();
    let mut cursor = 0;
    assert_eq!(unsigned(section, &mut cursor), 3);
    for parameters in [4, guard_parameters, store_parameters] {
        assert_eq!(section[cursor], 0x60);
        cursor += 1;
        assert_eq!(unsigned(section, &mut cursor), parameters);
        assert_eq!(
            &section[cursor..cursor + parameters],
            vec![0x7f; parameters]
        );
        cursor += parameters;
        assert_eq!(unsigned(section, &mut cursor), 1);
        assert_eq!(section[cursor], 0x7f);
        cursor += 1;
    }
    assert_eq!(cursor, section.len());
    let section = sections[2].unwrap();
    let mut cursor = 0;
    assert_eq!(unsigned(section, &mut cursor), 3);
    assert_eq!(
        (name(section, &mut cursor), name(section, &mut cursor)),
        ("env".to_owned(), "memory".to_owned())
    );
    assert_eq!(section[cursor], 2);
    cursor += 1;
    assert_eq!(unsigned(section, &mut cursor), 0);
    assert_eq!(unsigned(section, &mut cursor), 1);
    let guard_name = match owner {
        Owner::Replacement => "guard",
        Owner::Resident => "guard_resident",
    };
    let store_name = match owner {
        Owner::Replacement => "store8",
        Owner::Resident => "store_resident8",
    };
    for (expected_name, type_index) in [(guard_name, 1), (store_name, 2)] {
        assert_eq!(
            (name(section, &mut cursor), name(section, &mut cursor)),
            ("ring3".to_owned(), expected_name.to_owned())
        );
        assert_eq!(section[cursor], 0);
        cursor += 1;
        assert_eq!(unsigned(section, &mut cursor), type_index);
    }
    assert_eq!(cursor, section.len());
    let section = sections[3].unwrap();
    let mut cursor = 0;
    assert_eq!(unsigned(section, &mut cursor), 1);
    assert_eq!(unsigned(section, &mut cursor), 0);
    assert_eq!(cursor, section.len());
    assert!(sections[4].is_none() && sections[5].is_none() && sections[6].is_none());
    let section = sections[7].unwrap();
    let mut cursor = 0;
    assert_eq!(unsigned(section, &mut cursor), 1);
    assert_eq!(name(section, &mut cursor), "run");
    assert_eq!(section[cursor], 0);
    cursor += 1;
    assert_eq!(unsigned(section, &mut cursor), 2);
    assert_eq!(cursor, section.len());
    let section = sections[10].unwrap();
    let mut cursor = 0;
    assert_eq!(unsigned(section, &mut cursor), 1);
    let length = unsigned(section, &mut cursor);
    assert_eq!(cursor + length, section.len());
    assert_eq!(unsigned(section, &mut cursor), 3);
    for (count, kind) in [(16, 0x7f), (1, 0x7e), (6, 0x7f)] {
        assert_eq!(unsigned(section, &mut cursor), count);
        assert_eq!(section[cursor], kind);
        cursor += 1;
    }
    assert_eq!(section.last(), Some(&0x0b));
    assert!(
        sections[8].is_none()
            && sections[9].is_none()
            && sections[11].is_none()
            && sections[12].is_none()
    );
}

fn admit(forms: &[(Vec<u8>, Operation)]) {
    let mut bytes = Vec::new();
    let mut starts = Vec::new();
    for (instruction, _) in forms {
        starts.push(CODE + bytes.len() as u32);
        bytes.extend(instruction);
    }
    starts.push(CODE + bytes.len() as u32);
    bytes.extend([0xeb, 0]);
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut modules = Vec::new();
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let pointer = engine.arena_address();
            let mut whole_page = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(CODE), &mut whole_page)
                .unwrap();
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(CODE), bytes.len())
                .unwrap();
            let id = compile(&mut engine, owner, entries).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.arena_address(), pointer);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0xa5a5_a5a5), Access::Read)
                    .is_err()
            );
            let mut code_bytes = vec![0; bytes.len()];
            engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(CODE), &mut code_bytes)
                .unwrap();
            assert_eq!(code_bytes, bytes);
            let mut observed_page = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(CODE), &mut observed_page)
                .unwrap();
            assert_eq!(observed_page, whole_page);
            guard(&engine, owner, id).unwrap();
            let artifact = match owner {
                Owner::Replacement => engine.artifact_bytes().unwrap(),
                Owner::Resident => {
                    for &pc in &starts {
                        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                    }
                    assert!(engine.lookup_resident(CODE + 1).is_err());
                    assert_eq!(engine.generation(), 0);
                    engine.resident_bytes(id).unwrap()
                }
            };
            assert_eq!(&artifact[..8], b"\0asm\x01\0\0\0");
            assert!(artifact.len() <= CompileLimits::default().wasm_bytes);
            store_only_declarations(artifact, owner);
            modules.push((id, artifact.to_vec()));
        }
        match owner {
            Owner::Replacement => {
                assert_eq!(modules[0].0, modules[1].0);
                assert_eq!(modules[0].1, modules[1].1);
            }
            Owner::Resident => {
                // public resident units have unique process ids baked into their guards.
                assert_ne!(modules[0].0, modules[1].0);
                assert_ne!(modules[0].1, modules[1].1);
            }
        }
    }
}

#[test]
fn four_bound_profiles_need_only_store8_and_bare_compilation_refuses() {
    let mut bytes = encoding(4, 0, &[0x03]);
    bytes.extend([0xeb, 0]);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let specs = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: bytes.len() as u32,
    }];
    let error = instruction_error(CODE, InstructionError::BackendUnsupported);
    for observed in [
        prepare_region(memory, &specs, CompileLimits::default()).err(),
        prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        compile_region(memory, &specs, CompileLimits::default()).err(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
    ] {
        assert_eq!(observed, Some(error), "{bytes:02x?}");
    }
    for batch in canonical_forms().chunks_exact(16) {
        admit(batch);
    }
    let mut selected = Vec::new();
    for (tail, address) in address_cases() {
        for index in [0, 15] {
            selected.push((
                encoding(index, 7, &tail),
                Operation::MemorySetByte {
                    condition: CONDITIONS[index as usize],
                    address,
                },
            ));
        }
    }
    assert_eq!(selected.len(), 16);
    admit(&selected);
}

fn excluded() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut forms = Vec::new();
    for condition in 0..16 {
        let mut bytes = vec![0x66];
        bytes.extend(encoding(condition, 0, &[0x03]));
        forms.push((bytes, opcode));
        for modrm in [0x03, 0xc0] {
            forms.push((
                vec![0xf0, 0x0f, 0x90 + condition, modrm],
                DecodeError::InvalidEncoding,
            ));
        }
    }
    for condition in [0, 15] {
        for prefix in [0x67, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(condition, 7, &[0x04, 0x24]));
            forms.push((bytes, opcode));
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(condition, 7, &[0x03]));
            forms.push((bytes, DecodeError::Unsupported(UnsupportedFeature::Segment)));
        }
    }
    for prefix in [0x66, 0x67, 0xf2, 0xf3] {
        forms.push((vec![prefix, 0x0f, 0x94, 0xc0], opcode));
    }
    for bytes in [
        &[0x86, 0x03][..],
        &[0x86, 0xc0],
        &[0x0f, 0xc0, 0x03],
        &[0x0f, 0xc0, 0xc0],
        &[0xc0, 0x23, 1],
        &[0xc0, 0x2b, 1],
        &[0xc0, 0x3b, 1],
        &[0xc0, 0x23, 0],
        &[0xc0, 0x2b, 0],
        &[0xc0, 0x3b, 0],
        &[0xd2, 0x23],
        &[0xd2, 0x2b],
        &[0xd2, 0x3b],
        &[0xd0, 0x03],
        &[0xf6, 0x0b, 0xff],
        &[0xf6, 0x23],
        &[0xf6, 0x2b],
        &[0xf6, 0x33],
        &[0xf6, 0x3b],
    ] {
        forms.push((bytes.to_vec(), opcode));
    }
    forms.push((vec![0xfe, 0x13], DecodeError::InvalidEncoding));
    assert_eq!(forms.len(), 90);
    forms
}

#[test]
fn excluded_width_prefix_lock_segment_and_adjacent_forms_keep_exact_categories() {
    for (bytes, expected) in excluded() {
        let engine = code(CODE, &bytes, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        let memory = engine.memory().unwrap();
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

fn fetch_forms() -> Vec<Vec<u8>> {
    vec![
        encoding(0, 0, &[0x03]),
        encoding(4, 7, &[0x41, 0x80]),
        encoding(15, 3, &[0x44, 0x54, 0x7f]),
        encoding(4, 7, &[0x05, 0xff, 0xff, 0xff, 0xff]),
        encoding(0, 0, &[0x84, 0x94, 0x20, 0, 0, 0]),
    ]
}

#[test]
fn exact_fetch_wrap_and_cross_page_snapshots_keep_their_boundaries() {
    for bytes in fetch_forms() {
        let length = bytes.len() as u32;
        let pc = 0x2000 - length;
        let engine = code(pc, &bytes, true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(decoded.length() as u32, length);
        assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        let pc = u32::MAX - (length - 1);
        let engine = code(pc, &bytes, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc))
                .unwrap()
                .next_pc(),
            GuestAddress(0)
        );
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            let engine = code(pc, &bytes[..cut], true);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    0x2000,
                    cut as u32 + 1,
                    FaultReason::Unmapped
                ))
            );
            let mut engine = code(pc, &bytes, false);
            engine.protect(0x1000, 1, 4).unwrap();
            engine.protect(0x2000, 1, 1).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    0x2000,
                    cut as u32 + 1,
                    FaultReason::Permission
                ))
            );
            let pc = u32::MAX - (cut as u32 - 1);
            let engine = code(pc, &bytes[..cut], true);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(fetch_error(
                    pc,
                    pc,
                    cut as u32 + 1,
                    FaultReason::AddressOverflow
                ))
            );
        }
    }
    let pc = 0x1ffc;
    let mut bytes = encoding(4, 7, &[0x84, 0x94, 0x20, 0, 0, 0]);
    bytes.extend([0xeb, 0]);
    for changed in [0x1fff, 0x2000] {
        for mutation in 0..4 {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes, false);
                    engine.map(KEEP, 1, 7).unwrap();
                    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                    engine.map(DATA, 1, 3).unwrap();
                    upload(&mut engine, DATA, &[0xa7; 32]);
                    describe(&mut engine, KEEP, 3, false);
                    let keep = compile(&mut engine, Owner::Resident, false).unwrap();
                    let keeper = engine.resident_bytes(keep).unwrap();
                    let keeper_before = (keeper.to_vec(), keeper.as_ptr() as usize);
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                    describe(&mut engine, pc, bytes.len(), entries);
                    let id = compile(&mut engine, owner, entries).unwrap();
                    engine.protect(DATA, 1, 1).unwrap();
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    guard(&engine, owner, id).unwrap();
                    let page = changed & !0xfff;
                    let mut page_before = vec![0; 4096];
                    engine
                        .memory()
                        .unwrap()
                        .read(GuestAddress(page), &mut page_before)
                        .unwrap();
                    let original = bytes[(changed - pc) as usize];
                    match mutation {
                        0 => engine.write8(changed, u32::from(original)).unwrap(),
                        1 => engine.write8(changed, u32::from(original ^ 1)).unwrap(),
                        2 => engine.protect(page, 1, 7).unwrap(),
                        3 => {
                            engine.unmap(page, 1).unwrap();
                            engine.map(page, 1, 7).unwrap();
                            upload(&mut engine, page, &page_before);
                        }
                        _ => unreachable!(),
                    }
                    let after = engine.arena().to_vec();
                    let arena_pointer = engine.arena_address();
                    assert!(
                        !engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    let expected = match owner {
                        Owner::Replacement => HostError::CodeInvalidated,
                        Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
                    };
                    assert_eq!(guard(&engine, owner, id), Err(expected));
                    let observed = match owner {
                        Owner::Replacement => engine.artifact_bytes(),
                        Owner::Resident => engine.resident_bytes(id),
                    };
                    assert_eq!(observed, Err(expected));
                    guard(&engine, Owner::Resident, keep).unwrap();
                    let keeper = engine.resident_bytes(keep).unwrap();
                    assert_eq!(keeper, keeper_before.0);
                    assert_eq!(keeper.as_ptr() as usize, keeper_before.1);
                    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                    assert_eq!(engine.arena(), after);
                    assert_eq!(engine.arena_address(), arena_pointer);
                    let mut expected_page = page_before;
                    if mutation == 1 {
                        expected_page[(changed - page) as usize] ^= 1;
                    }
                    let mut page_after = vec![0; 4096];
                    engine
                        .memory()
                        .unwrap()
                        .read(GuestAddress(page), &mut page_after)
                        .unwrap();
                    assert_eq!(page_after, expected_page);
                    let mut data = vec![0; 4096];
                    engine
                        .memory()
                        .unwrap()
                        .read(GuestAddress(DATA), &mut data)
                        .unwrap();
                    assert_eq!(&data[..32], &[0xa7; 32]);
                    assert!(data[32..].iter().all(|&value| value == 0));
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    replacement: (Vec<u8>, usize),
    resident: (Vec<u8>, usize),
    dispatcher: (Vec<u8>, usize),
    ram: [Vec<u8>; 3],
    mapped_pages: u32,
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    let retain = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    Publication {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        replacement: retain(engine.artifact_bytes().unwrap()),
        resident: retain(engine.resident_bytes(id).unwrap()),
        dispatcher: retain(engine.dispatcher_bytes(KEY).unwrap()),
        ram: [CODE, KEEP, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        mapped_pages: engine.memory().unwrap().mapped_pages(),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = code(CODE, &[0x90], false);
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    engine.map(DATA, 1, 3).unwrap();
    engine.write32(DATA, 0x9234_5678).unwrap();
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, id, 3)
        .unwrap();
    (engine, id)
}

fn failed(
    engine: &mut EngineInstance,
    keep: u64,
    owner: Owner,
    entries: bool,
    pc: u32,
    length: usize,
    expected: CompileError,
) {
    describe(engine, pc, length, entries);
    let before = publication(engine, keep);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(CODE), 4096)
        .unwrap();
    assert_eq!(
        compile(engine, owner, entries),
        Err(host_error(owner, expected))
    );
    assert_eq!(publication(engine, keep), before);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.lookup_resident(CODE).is_err());
}

#[test]
fn family_caps_and_late_failures_preserve_all_publications() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    let registry_limits = RegistryLimits::default();
    assert_eq!(
        (registry_limits.units, registry_limits.wasm_bytes),
        (8, 8 * 65_536)
    );
    let mut bytes = encoding(4, 0, &[0x03]);
    bytes.extend([0x90; 62]);
    bytes.extend([0xeb, 0]);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, entries).unwrap();
            assert_eq!(engine.arena(), before);
            guard(&engine, owner, id).unwrap();
            let module = match owner {
                Owner::Replacement => engine.artifact_bytes().unwrap(),
                Owner::Resident => engine.resident_bytes(id).unwrap(),
            };
            assert!(module.len() <= 65_536);
            store_only_declarations(module, owner);
        }
    }
    let block = [0x0f, 0x94, 0x03, 0xeb, 0];
    let bytes = block.repeat(8);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            let transfer = &mut engine.arena_mut().unwrap()[140..];
            transfer.fill(0xa5);
            for index in 0..8 {
                let stride = if entries { 4 } else { 8 };
                let at = index * stride;
                transfer[at..at + 4].copy_from_slice(&(CODE + index as u32 * 5).to_le_bytes());
                if !entries {
                    transfer[at + 4..at + 8].copy_from_slice(&5_u32.to_le_bytes());
                }
            }
            let before = engine.arena().to_vec();
            let id = compile_count(&mut engine, owner, entries, 8).unwrap();
            assert_eq!(engine.arena(), before);
            guard(&engine, owner, id).unwrap();
            assert_eq!(
                compile_count(&mut engine, owner, entries, 9),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            guard(&engine, owner, id).unwrap();
        }
    }
    // every consumed code byte and declared data byte precedes the first publication.
    for entries in [false, true] {
        let mut engine = code(CODE, &block.repeat(9), true);
        engine.map(DATA, 1, 3).unwrap();
        upload(&mut engine, DATA, &[0xa7; 32]);
        let mut ids = Vec::new();
        for index in 0..8 {
            describe(&mut engine, CODE + index * 5, 5, entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, Owner::Resident, entries).unwrap();
            assert!(!ids.contains(&id));
            ids.push(id);
            assert_eq!(engine.arena(), before);
            if index == 0 {
                engine
                    .acknowledge_resident_installation(KEY, id, 3)
                    .unwrap();
            }
        }
        let retained: Vec<_> = ids
            .iter()
            .map(|&id| {
                let bytes = engine.resident_bytes(id).unwrap();
                (bytes.to_vec(), bytes.as_ptr() as usize)
            })
            .collect();
        let dispatcher = engine.dispatcher_bytes(KEY).unwrap();
        let dispatcher_before = (dispatcher.to_vec(), dispatcher.as_ptr() as usize);
        let installed = engine.lookup_installed_resident(KEY, CODE).unwrap();
        let memory_before = [CODE, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            if pc == CODE {
                engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(pc), &mut bytes)
                    .unwrap();
            } else {
                engine
                    .memory()
                    .unwrap()
                    .read(GuestAddress(pc), &mut bytes)
                    .unwrap();
            }
            bytes
        });
        describe(&mut engine, CODE + 40, 5, entries);
        let before = engine.arena().to_vec();
        let arena_pointer = engine.arena_address();
        assert_eq!(
            compile(&mut engine, Owner::Resident, entries),
            Err(HostError::Resident(RegistryError::UnitCapacity))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.arena_address(), arena_pointer);
        assert_eq!(
            engine.lookup_installed_resident(KEY, CODE).unwrap(),
            installed
        );
        let dispatcher = engine.dispatcher_bytes(KEY).unwrap();
        assert_eq!(dispatcher, dispatcher_before.0);
        assert_eq!(dispatcher.as_ptr() as usize, dispatcher_before.1);
        assert_eq!(engine.generation(), 0);
        for (index, (&id, (bytes, pointer))) in ids.iter().zip(retained).enumerate() {
            let observed = engine.resident_bytes(id).unwrap();
            assert_eq!(observed, bytes);
            assert_eq!(observed.as_ptr() as usize, pointer);
            guard(&engine, Owner::Resident, id).unwrap();
            assert_eq!(
                engine
                    .lookup_resident(CODE + index as u32 * 5)
                    .unwrap()
                    .get(),
                id
            );
        }
        assert!(engine.lookup_resident(CODE + 40).is_err());
        assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
        for (pc, expected) in [CODE, DATA].into_iter().zip(memory_before) {
            let mut bytes = vec![0; 4096];
            if pc == CODE {
                engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(pc), &mut bytes)
                    .unwrap();
            } else {
                engine
                    .memory()
                    .unwrap()
                    .read(GuestAddress(pc), &mut bytes)
                    .unwrap();
            }
            assert_eq!(bytes, expected);
        }
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners();
            for (instruction, error) in excluded() {
                let mut bytes = encoding(4, 0, &[0x06]);
                bytes.extend(instruction);
                upload(&mut engine, CODE, &bytes);
                failed(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    CODE,
                    bytes.len(),
                    instruction_error(CODE + 3, InstructionError::Decode(error)),
                );
            }
            let instruction = encoding(4, 7, &[0x84, 0x94, 0x20, 0, 0, 0]);
            for cut in [1, instruction.len() - 1] {
                let pc = 0x2000 - cut as u32;
                upload(&mut engine, pc, &instruction[..cut]);
                failed(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    pc,
                    instruction.len(),
                    instruction_error(
                        pc,
                        InstructionError::Decode(fetch_error(
                            pc,
                            0x2000,
                            cut as u32 + 1,
                            FaultReason::Unmapped,
                        )),
                    ),
                );
            }
            if !entries {
                let mut bytes = encoding(4, 0, &[0x06]);
                bytes.extend(&instruction);
                upload(&mut engine, CODE, &bytes);
                failed(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    CODE,
                    bytes.len() - 1,
                    instruction_error(CODE + 3, InstructionError::InvalidBlockEnd),
                );
            }
            let mut bytes = encoding(4, 0, &[0x06]);
            bytes.extend([0x90; 63]);
            bytes.extend([0xeb, 0]);
            upload(&mut engine, CODE, &bytes);
            failed(
                &mut engine,
                keep,
                owner,
                entries,
                CODE,
                bytes.len(),
                CompileError::InstructionLimit,
            );
        }
    }
}
