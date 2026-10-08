use ring3_engine::process::EngineInstance;

#[test]
fn register_memory_byte_arithmetic_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x02, 0x06, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register byte arithmetic from memory must admit in the bound engine");
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
            ir::{ByteReadArithmeticKind, ByteRegister, EffectiveAddress, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{HostError, ResidentInstallation},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x8abc_def0_f123_4567;
const REGISTERS: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];
const FAMILIES: [(ByteReadArithmeticKind, u8, u8, Option<u8>); 2] = [
    (ByteReadArithmeticKind::Add, 0x02, 0, None),
    (ByteReadArithmeticKind::Sub, 0x2a, 0, None),
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

fn encoding(opcode: u8, field: u8, tail: &[u8], immediate: Option<u8>) -> Vec<u8> {
    assert_eq!(tail[0] & 0x38, 0);
    assert_ne!(tail[0] & 0xc0, 0xc0);
    let mut bytes = vec![opcode, tail[0] | field << 3];
    bytes.extend_from_slice(&tail[1..]);
    if let Some(value) = immediate {
        bytes.push(value);
    }
    bytes
}

fn canonical_forms() -> Vec<(Vec<u8>, Operation)> {
    let mut forms = Vec::new();
    for (kind, opcode, _, _) in FAMILIES {
        for (field, destination) in REGISTERS.into_iter().enumerate() {
            forms.push((
                encoding(opcode, field as u8, &[0x06], None),
                Operation::ReadArithmeticByte {
                    kind,
                    destination,
                    address: address(Some(Register32::Esi), None, 1, 0),
                },
            ));
        }
    }
    assert_eq!(forms.len(), 16);
    forms
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

fn address_forms() -> Vec<(Vec<u8>, Operation)> {
    let cases: &[(&[u8], ByteRegister, EffectiveAddress)] = &[
        (
            &[0x00],
            ByteRegister::Dl,
            address(Some(Register32::Eax), None, 1, 0),
        ),
        (
            &[0x01],
            ByteRegister::Bl,
            address(Some(Register32::Ecx), None, 1, 0),
        ),
        (
            &[0x02],
            ByteRegister::Ah,
            address(Some(Register32::Edx), None, 1, 0),
        ),
        (
            &[0x03],
            ByteRegister::Ch,
            address(Some(Register32::Ebx), None, 1, 0),
        ),
        (
            &[0x45, 0],
            ByteRegister::Dh,
            address(Some(Register32::Ebp), None, 1, 0),
        ),
        (
            &[0x04, 0x24],
            ByteRegister::Bh,
            address(Some(Register32::Esp), None, 1, 0),
        ),
        (
            &[0x04, 0x0b],
            ByteRegister::Al,
            address(Some(Register32::Ebx), Some(Register32::Ecx), 1, 0),
        ),
        (
            &[0x04, 0x4b],
            ByteRegister::Cl,
            address(Some(Register32::Ebx), Some(Register32::Ecx), 2, 0),
        ),
        (
            &[0x04, 0x8b],
            ByteRegister::Dl,
            address(Some(Register32::Ebx), Some(Register32::Ecx), 4, 0),
        ),
        (
            &[0x44, 0xcb, 0x80],
            ByteRegister::Bl,
            address(
                Some(Register32::Ebx),
                Some(Register32::Ecx),
                8,
                (-128_i32) as u32,
            ),
        ),
        (
            &[0x04, 0x8d, 0xf8, 0xff, 0xff, 0xff],
            ByteRegister::Ah,
            address(None, Some(Register32::Ecx), 4, (-8_i32) as u32),
        ),
        (
            &[0x05, 0xff, 0xff, 0xff, 0xff],
            ByteRegister::Ch,
            address(None, None, 1, u32::MAX),
        ),
        (
            &[0x43, 0x80],
            ByteRegister::Dh,
            address(Some(Register32::Ebx), None, 1, (-128_i32) as u32),
        ),
        (
            &[0x05, 0x78, 0x56, 0x34, 0x12],
            ByteRegister::Bh,
            address(None, None, 1, 0x1234_5678),
        ),
        (
            &[0x85, 0xe0, 0xff, 0xff, 0xff],
            ByteRegister::Al,
            address(Some(Register32::Ebp), None, 1, (-32_i32) as u32),
        ),
        (
            &[0x86, 0x78, 0x56, 0x34, 0x92],
            ByteRegister::Cl,
            address(Some(Register32::Esi), None, 1, 0x9234_5678),
        ),
        (
            &[0x00],
            ByteRegister::Al,
            address(Some(Register32::Eax), None, 1, 0),
        ),
        (
            &[0x04, 0x45, 1, 0, 0, 0],
            ByteRegister::Ah,
            address(None, Some(Register32::Eax), 2, 1),
        ),
        (
            &[0x41, 0xf0],
            ByteRegister::Cl,
            address(Some(Register32::Ecx), None, 1, (-16_i32) as u32),
        ),
        (
            &[0x04, 0x4d, 1, 0, 0, 0],
            ByteRegister::Ch,
            address(None, Some(Register32::Ecx), 2, 1),
        ),
        (
            &[0x44, 0x54, 0x11],
            ByteRegister::Dl,
            address(Some(Register32::Esp), Some(Register32::Edx), 2, 17),
        ),
        (
            &[0x44, 0x95, 0],
            ByteRegister::Dh,
            address(Some(Register32::Ebp), Some(Register32::Edx), 4, 0),
        ),
        (
            &[0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff],
            ByteRegister::Bl,
            address(
                Some(Register32::Ebx),
                Some(Register32::Esi),
                8,
                (-32_i32) as u32,
            ),
        ),
        (
            &[0x83, 0, 1, 0, 0],
            ByteRegister::Bh,
            address(Some(Register32::Ebx), None, 1, 256),
        ),
        (
            &[0x05, 0, 0, 0, 0],
            ByteRegister::Al,
            address(None, None, 1, 0),
        ),
        (
            &[0x05, 0xff, 0x1f, 0, 0],
            ByteRegister::Cl,
            address(None, None, 1, 0x1fff),
        ),
        (
            &[0x40, 0x7f],
            ByteRegister::Dl,
            address(Some(Register32::Eax), None, 1, 127),
        ),
        (
            &[0x83, 0xff, 0xff, 0xff, 0xff],
            ByteRegister::Bl,
            address(Some(Register32::Ebx), None, 1, u32::MAX),
        ),
        (
            &[0x44, 0xd5, 0x80],
            ByteRegister::Ah,
            address(
                Some(Register32::Ebp),
                Some(Register32::Edx),
                8,
                (-128_i32) as u32,
            ),
        ),
        (
            &[0x44, 0xb1, 0x7f],
            ByteRegister::Ch,
            address(Some(Register32::Ecx), Some(Register32::Esi), 4, 127),
        ),
        (
            &[0x84, 0xfe, 0xe0, 0xff, 0xff, 0xff],
            ByteRegister::Dh,
            address(
                Some(Register32::Esi),
                Some(Register32::Edi),
                8,
                (-32_i32) as u32,
            ),
        ),
        (
            &[0x84, 0x94, 0x20, 0, 0, 0],
            ByteRegister::Bh,
            address(Some(Register32::Esp), Some(Register32::Edx), 4, 32),
        ),
    ];
    assert_eq!(cases.len(), 32);
    let mut forms = Vec::new();
    for (kind, opcode, _, _) in FAMILIES {
        for &(tail, destination, address) in cases {
            let field = REGISTERS
                .iter()
                .position(|&register| register == destination)
                .unwrap();
            forms.push((
                encoding(opcode, field as u8, tail, None),
                Operation::ReadArithmeticByte {
                    kind,
                    destination,
                    address,
                },
            ));
        }
    }
    assert_eq!(forms.len(), 64);
    forms
}

#[test]
fn two_families_decode_all_byte_destinations_and_flat32_addresses() {
    let mut engine = code(CODE, &[0x90], false);
    for (bytes, expected) in canonical_forms().into_iter().chain(address_forms()) {
        upload(&mut engine, CODE, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
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

fn imported_names(bytes: &[u8]) -> Vec<(String, String)> {
    let mut cursor = 8;
    while cursor < bytes.len() {
        let section = bytes[cursor];
        cursor += 1;
        let length = unsigned(bytes, &mut cursor);
        let end = cursor + length;
        if section == 2 {
            let count = unsigned(bytes, &mut cursor);
            let mut names = Vec::new();
            for _ in 0..count {
                let mut strings = Vec::new();
                for _ in 0..2 {
                    let length = unsigned(bytes, &mut cursor);
                    strings.push(
                        std::str::from_utf8(&bytes[cursor..cursor + length])
                            .unwrap()
                            .to_owned(),
                    );
                    cursor += length;
                }
                names.push((strings.remove(0), strings.remove(0)));
                let kind = bytes[cursor];
                cursor += 1;
                match kind {
                    0 => {
                        unsigned(bytes, &mut cursor);
                    }
                    2 => {
                        let flags = unsigned(bytes, &mut cursor);
                        assert_eq!(flags, 0);
                        assert_eq!(unsigned(bytes, &mut cursor), 1);
                    }
                    _ => panic!("unexpected generated import kind {kind}"),
                }
            }
            assert_eq!(cursor, end);
            return names;
        }
        cursor = end;
    }
    panic!("bound generated module has no import section");
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
            let guard_name = match owner {
                Owner::Replacement => "guard",
                Owner::Resident => "guard_resident",
            };
            assert_eq!(
                imported_names(artifact),
                vec![
                    ("env".to_owned(), "memory".to_owned()),
                    ("ring3".to_owned(), guard_name.to_owned()),
                    ("ring3".to_owned(), "read8".to_owned()),
                ]
            );
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
fn canonical_batches_require_bound_read8_only_in_four_profiles_with_existing_caps() {
    let compile_limits = CompileLimits::default();
    assert_eq!(
        (
            compile_limits.instructions,
            compile_limits.blocks,
            compile_limits.wasm_bytes
        ),
        (64, 8, 65_536)
    );
    let registry_limits = RegistryLimits::default();
    assert_eq!(
        (registry_limits.units, registry_limits.wasm_bytes),
        (8, 8 * 65_536)
    );
    for (kind, opcode, field, immediate) in FAMILIES {
        let mut bytes = encoding(opcode, field, &[0x03], immediate);
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
            assert_eq!(observed, Some(error), "{kind:?} {bytes:02x?}");
        }
    }
    for family in canonical_forms().chunks_exact(8) {
        admit(family);
    }
    let forms = address_forms();
    for family in forms.chunks_exact(16) {
        admit(family);
    }
    // one memory operation keeps the positive instruction cap independent of helper size.
    let mut bytes = vec![0x02, 0x03];
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
        }
    }
    let bytes = [0x02, 0x03, 0xeb, 0].repeat(8);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            let transfer = &mut engine.arena_mut().unwrap()[140..];
            transfer.fill(0xa5);
            for index in 0..8 {
                let stride = if entries { 4 } else { 8 };
                let at = index * stride;
                transfer[at..at + 4].copy_from_slice(&(CODE + index as u32 * 4).to_le_bytes());
                if !entries {
                    transfer[at + 4..at + 8].copy_from_slice(&4_u32.to_le_bytes());
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
    for entries in [false, true] {
        let mut engine = code(CODE, &[0x02, 0x03, 0xeb, 0].repeat(9), true);
        engine.map(DATA, 1, 3).unwrap();
        engine.write32(DATA, 0x9234_5678).unwrap();
        let mut ids = Vec::new();
        for index in 0..8 {
            describe(&mut engine, CODE + index * 4, 4, entries);
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
        describe(&mut engine, CODE + 32, 4, entries);
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
                    .lookup_resident(CODE + index as u32 * 4)
                    .unwrap()
                    .get(),
                id
            );
        }
        assert!(engine.lookup_resident(CODE + 32).is_err());
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
}

fn excluded() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut forms = Vec::new();
    for (_, instruction_opcode, _, _) in FAMILIES {
        let instruction = encoding(instruction_opcode, 0, &[0x03], None);
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(&instruction);
            forms.push((bytes, opcode));
        }
        forms.push((vec![0x66, instruction_opcode + 1, 0x03], opcode));
        for modrm in [0x03, 0xc0] {
            forms.push((
                vec![0xf0, instruction_opcode, modrm],
                DecodeError::InvalidEncoding,
            ));
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let mut bytes = vec![prefix];
            bytes.extend(&instruction);
            forms.push((bytes, DecodeError::Unsupported(UnsupportedFeature::Segment)));
        }
    }
    for group in [0, 5] {
        for modrm in [0x03 | group << 3, 0xc3 | group << 3] {
            forms.push((vec![0x82, modrm, 0xff], opcode));
        }
    }
    for bytes in [
        &[0x66, 0x12, 0x03][..],
        &[0x66, 0x1a, 0x03],
        &[0x66, 0x22, 0x03],
        &[0x66, 0x0a, 0x03],
        &[0x66, 0x32, 0x03],
        &[0x66, 0x3a, 0x03],
        &[0x86, 0x03],
        &[0x66, 0x0f, 0xc0, 0x03],
        &[0x66, 0xc0, 0x23, 2],
        &[0x66, 0xc0, 0x2b, 2],
        &[0x66, 0xc0, 0x3b, 2],
        &[0x66, 0xc0, 0x23, 0],
        &[0x66, 0xc0, 0x2b, 0],
        &[0x66, 0xc0, 0x3b, 0],
        &[0x66, 0xd2, 0x23],
        &[0x66, 0xd2, 0x2b],
        &[0x66, 0xd2, 0x3b],
        &[0x66, 0xd0, 0x03],
        &[0x66, 0xd0, 0x0b],
        &[0x66, 0xd0, 0x13],
        &[0x66, 0xd0, 0x1b],
        &[0xd0, 0x33],
        &[0xf6, 0x0b, 0xff],
        &[0x66, 0xf6, 0x23],
        &[0x66, 0xf6, 0x2b],
        &[0x66, 0xf6, 0x33],
        &[0x66, 0xf6, 0x3b],
    ] {
        forms.push((bytes.to_vec(), opcode));
    }
    forms.push((vec![0xfe, 0x13], DecodeError::InvalidEncoding));
    assert_eq!(forms.len(), 58);
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
    FAMILIES
        .into_iter()
        .flat_map(|(_, opcode, field, immediate)| {
            [
                encoding(opcode, field, &[0x03], immediate),
                encoding(opcode, field, &[0x45, 0x80], immediate),
                encoding(opcode, field, &[0x44, 0x8b, 0x80], immediate),
                encoding(opcode, field, &[0x05, 0xff, 0xff, 0xff, 0xff], immediate),
                encoding(
                    opcode,
                    field,
                    &[0x84, 0x8b, 0x78, 0x56, 0x34, 0x92],
                    immediate,
                ),
            ]
        })
        .collect()
}

#[test]
fn exact_fetch_wrap_and_consumed_page_currency_keep_their_boundaries() {
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
    for (_, opcode, field, immediate) in FAMILIES {
        let pc = 0x1ffc;
        let mut bytes = encoding(
            opcode,
            field,
            &[0x84, 0x8b, 0x78, 0x56, 0x34, 0x92],
            immediate,
        );
        bytes.extend([0xeb, 0]);
        for changed in [0x1fff, 0x2000] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes, false);
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                    describe(&mut engine, pc, bytes.len(), entries);
                    let id = compile(&mut engine, owner, entries).unwrap();
                    engine.map(DATA, 1, 3).unwrap();
                    engine.write32(DATA, 0x9234_5678).unwrap();
                    engine.protect(DATA, 1, 1).unwrap();
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    guard(&engine, owner, id).unwrap();
                    engine
                        .write8(changed, u32::from(bytes[(changed - pc) as usize]))
                        .unwrap();
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
fn failed_late_decode_fetch_extent_and_cap_preparation_preserves_both_published_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners();
            for (instruction, error) in excluded() {
                let mut bytes = vec![0x02, 0x06];
                bytes.extend(instruction);
                upload(&mut engine, CODE, &bytes);
                failed(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    CODE,
                    bytes.len(),
                    instruction_error(CODE + 2, InstructionError::Decode(error)),
                );
            }
            for (_, opcode, field, immediate) in FAMILIES {
                let instruction = encoding(
                    opcode,
                    field,
                    &[0x84, 0x8b, 0x78, 0x56, 0x34, 0x92],
                    immediate,
                );
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
                    let mut bytes = vec![0x02, 0x06];
                    bytes.extend(&instruction);
                    upload(&mut engine, CODE, &bytes);
                    failed(
                        &mut engine,
                        keep,
                        owner,
                        entries,
                        CODE,
                        bytes.len() - 1,
                        instruction_error(CODE + 2, InstructionError::InvalidBlockEnd),
                    );
                }
            }
            let mut bytes = vec![0x02, 0x06];
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
