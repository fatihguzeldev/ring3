use ring3_engine::process::EngineInstance;

const KEY: u64 = 0x1234_5678_9abc_def0;

#[test]
fn bound_memory_shld_admits_without_reading_unmapped_data() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let code = [0x0f, 0xa4, 0x13, 0x01, 0xeb, 0x00];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..146].copy_from_slice(&code);
    engine.upload(0x1000, 6).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&6_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("bound memory SHLD32 must admit without reading guest data");
}

use std::{
    collections::BTreeSet,
    io::Write,
    process::{Command, Stdio},
};

use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{DoubleShiftKind, EffectiveAddress, Operation, ShiftCount},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
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
const FORMS: [(DoubleShiftKind, u8); 4] = [
    (DoubleShiftKind::Left, 0xa4),
    (DoubleShiftKind::Left, 0xa5),
    (DoubleShiftKind::Right, 0xac),
    (DoubleShiftKind::Right, 0xad),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Replacement,
    Resident,
}

#[derive(Clone, Debug)]
struct Shape {
    modrm: u8,
    suffix: Vec<u8>,
    address: EffectiveAddress,
}

fn shapes() -> Vec<Shape> {
    vec![
        Shape {
            modrm: 0x05,
            suffix: vec![0x10, 0x50, 0, 0],
            address: EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: DATA + 0x10,
            },
        },
        Shape {
            modrm: 0x00,
            suffix: vec![],
            address: EffectiveAddress {
                base: Some(Register32::Eax),
                index: None,
                scale: 1,
                displacement: 0,
            },
        },
        Shape {
            modrm: 0x44,
            suffix: vec![0x24, 8],
            address: EffectiveAddress {
                base: Some(Register32::Esp),
                index: None,
                scale: 1,
                displacement: 8,
            },
        },
        Shape {
            modrm: 0x45,
            suffix: vec![0],
            address: EffectiveAddress {
                base: Some(Register32::Ebp),
                index: None,
                scale: 1,
                displacement: 0,
            },
        },
        Shape {
            modrm: 0x43,
            suffix: vec![0x80],
            address: EffectiveAddress {
                base: Some(Register32::Ebx),
                index: None,
                scale: 1,
                displacement: 0xffff_ff80,
            },
        },
        Shape {
            modrm: 0x44,
            suffix: vec![0x89, 0xf0],
            address: EffectiveAddress {
                base: Some(Register32::Ecx),
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0xffff_fff0,
            },
        },
        Shape {
            modrm: 0x04,
            suffix: vec![0xd5, 0xf8, 0xff, 0xff, 0xff],
            address: EffectiveAddress {
                base: None,
                index: Some(Register32::Edx),
                scale: 8,
                displacement: 0xffff_fff8,
            },
        },
    ]
}

fn instruction(opcode: u8, shape: &Shape, source: usize, raw: u8) -> Vec<u8> {
    assert!(source < 8 && shape.modrm & 0x38 == 0);
    let mut bytes = vec![0x0f, opcode, shape.modrm | (source as u8) << 3];
    bytes.extend_from_slice(&shape.suffix);
    if matches!(opcode, 0xa4 | 0xac) {
        bytes.push(raw);
    }
    bytes
}

fn bank(shape: &Shape, source: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (_, opcode) in FORMS {
        bytes.extend(instruction(
            opcode,
            shape,
            source,
            if opcode == 0xa4 { 0 } else { 32 },
        ));
    }
    bytes.extend([0xeb, 0]);
    bytes
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn spec(pc: u32, length: usize) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length as u32,
    }
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)], entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    for (index, &(pc, length)) in specs.iter().enumerate() {
        let offset = index * if entries { 4 } else { 8 };
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[offset + 4..offset + 8].copy_from_slice(&(length as u32).to_le_bytes());
        }
    }
}

fn compile(
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

fn module(engine: &EngineInstance, owner: Owner, id: u64) -> &[u8] {
    match owner {
        Owner::Replacement => engine.artifact_bytes().unwrap(),
        Owner::Resident => engine.resident_bytes(id).unwrap(),
    }
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
}

fn compile_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
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

fn page(memory: &AddressSpace, pc: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    memory.fetch(GuestAddress(pc), &mut bytes).unwrap();
    bytes
}

fn byte(bytes: &mut &[u8]) -> u8 {
    let (&value, remaining) = bytes.split_first().unwrap();
    *bytes = remaining;
    value
}

fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for index in 0..5 {
        let next = byte(bytes);
        if index == 4 {
            assert_eq!(next & 0xf0, 0);
        }
        value |= u32::from(next & 0x7f) << (index * 7);
        if next & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated unsigned LEB");
}

fn signed_word(bytes: &mut &[u8]) -> u32 {
    let mut value = 0_i64;
    for index in 0..5 {
        let next = byte(bytes);
        let bits = (index + 1) * 7;
        value |= i64::from(next & 0x7f) << (index * 7);
        if next & 0x80 == 0 {
            if next & 0x40 != 0 {
                value -= 1_i64 << bits;
            }
            assert!(i32::try_from(value).is_ok());
            return value as i32 as u32;
        }
    }
    panic!("unterminated signed LEB");
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let (value, remaining) = bytes.split_at(length);
    *bytes = remaining;
    value
}

fn name<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = unsigned(bytes) as usize;
    std::str::from_utf8(take(bytes, length)).unwrap()
}

fn section(wasm: &[u8], requested: u8) -> &[u8] {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut bytes = &wasm[8..];
    while !bytes.is_empty() {
        let kind = byte(&mut bytes);
        let length = unsigned(&mut bytes) as usize;
        let value = take(&mut bytes, length);
        if kind == requested {
            return value;
        }
    }
    panic!("missing section {requested}");
}

fn assert_rmw_module(engine: &EngineInstance, owner: Owner, id: u64) {
    let wasm = module(engine, owner, id);
    let resident = owner == Owner::Resident;
    let mut types = section(wasm, 1);
    assert_eq!(unsigned(&mut types), 4);
    for arity in [
        4,
        if resident { 7 } else { 6 },
        1,
        if resident { 6 } else { 2 },
    ] {
        assert_eq!(byte(&mut types), 0x60);
        assert_eq!(unsigned(&mut types), arity);
        assert_eq!(take(&mut types, arity as usize), vec![0x7f; arity as usize]);
        assert_eq!((unsigned(&mut types), byte(&mut types)), (1, 0x7f));
    }
    assert!(types.is_empty());
    let mut imports = section(wasm, 2);
    assert_eq!(unsigned(&mut imports), 4);
    assert_eq!((name(&mut imports), name(&mut imports)), ("env", "memory"));
    assert_eq!(
        (
            byte(&mut imports),
            unsigned(&mut imports),
            unsigned(&mut imports)
        ),
        (2, 0, 1)
    );
    for (index, expected) in [
        if resident { "guard_resident" } else { "guard" },
        "read32",
        if resident {
            "store_resident32"
        } else {
            "store32"
        },
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            (name(&mut imports), name(&mut imports)),
            ("ring3", expected)
        );
        assert_eq!(
            (byte(&mut imports), unsigned(&mut imports)),
            (0, index as u32 + 1)
        );
    }
    assert!(imports.is_empty());
    let mut functions = section(wasm, 3);
    assert_eq!((unsigned(&mut functions), unsigned(&mut functions)), (1, 0));
    assert!(functions.is_empty());
    let mut exports = section(wasm, 7);
    assert_eq!(unsigned(&mut exports), 1);
    assert_eq!(name(&mut exports), "run");
    assert_eq!((byte(&mut exports), unsigned(&mut exports)), (0, 3));
    assert!(exports.is_empty());
    let mut bodies = section(wasm, 10);
    assert_eq!(unsigned(&mut bodies), 1);
    let length = unsigned(&mut bodies) as usize;
    let mut body = take(&mut bodies, length);
    assert!(bodies.is_empty());
    assert_eq!(unsigned(&mut body), 3);
    for expected in [(16, 0x7f), (1, 0x7e), (6, 0x7f)] {
        assert_eq!((unsigned(&mut body), byte(&mut body)), expected);
    }
    let mut constants = vec![KEY as u32, (KEY >> 32) as u32, id as u32];
    if resident {
        constants.push((id >> 32) as u32);
    }
    for expected in constants {
        assert_eq!(byte(&mut body), 0x41);
        assert_eq!(signed_word(&mut body), expected);
    }
    for local in [0, 1, 3] {
        assert_eq!((byte(&mut body), unsigned(&mut body)), (0x20, local));
    }
    assert_eq!((byte(&mut body), unsigned(&mut body)), (0x10, 0));
}

fn validate_modules(modules: &[(Owner, Vec<u8>)]) {
    let mut input = Vec::new();
    for (owner, bytes) in modules {
        input.push(if *owner == Owner::Replacement { 1 } else { 2 });
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
    }
    // module validation never instantiates or executes guest code.
    let script = r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0, count = 0;
while (at < input.length) {
  assert.ok(at + 5 <= input.length);
  const owner = input[at], length = input.readUInt32LE(at + 1); at += 5;
  assert.ok(owner === 1 || owner === 2);
  assert.ok(length > 8 && at + length <= input.length);
  const bytes = input.subarray(at, at + length); at += length;
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module:'env', name:'memory', kind:'memory'},
    {module:'ring3', name:owner === 1 ? 'guard' : 'guard_resident', kind:'function'},
    {module:'ring3', name:'read32', kind:'function'},
    {module:'ring3', name:owner === 1 ? 'store32' : 'store_resident32', kind:'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run', kind:'function'}]);
  count++;
}
process.stdout.write(String(count));
"#;
    let mut child = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("existing Node/V8 must validate native-generated Wasm");
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, modules.len().to_string().as_bytes());
}

#[test]
fn seven_address_shapes_and_all_sources_preserve_raw_double_shift_identity() {
    let mut engine = code(CODE, &[0x90; 16], false);
    let mut identities = BTreeSet::new();
    for (shape_index, shape) in shapes().iter().enumerate() {
        for (source_index, source) in REGISTERS.into_iter().enumerate() {
            for (kind, opcode) in FORMS {
                let counts: Vec<u8> = if matches!(opcode, 0xa4 | 0xac) {
                    (0..=255).collect()
                } else {
                    vec![0]
                };
                for raw in counts {
                    let bytes = instruction(opcode, shape, source_index, raw);
                    upload(&mut engine, CODE, &bytes);
                    let memory = engine.memory().unwrap();
                    let decoded = decode_one(memory, GuestAddress(CODE)).unwrap();
                    let count = if matches!(opcode, 0xa4 | 0xac) {
                        ShiftCount::Immediate(raw)
                    } else {
                        ShiftCount::Cl
                    };
                    assert_eq!(
                        decoded.operation(),
                        &Operation::MemoryDoubleShift {
                            kind,
                            address: shape.address,
                            source,
                            count
                        }
                    );
                    assert_eq!(
                        (decoded.pc(), decoded.length(), decoded.next_pc()),
                        (
                            GuestAddress(CODE),
                            bytes.len() as u8,
                            GuestAddress(CODE + bytes.len() as u32)
                        )
                    );
                    let mut consumed = vec![0; decoded.length() as usize];
                    memory.fetch(GuestAddress(CODE), &mut consumed).unwrap();
                    assert_eq!(consumed, bytes);
                    assert!(memory.is_code_current(decoded.code_snapshot()));
                    assert!(identities.insert((shape_index, source_index, opcode, raw)));
                }
            }
        }
    }
    assert_eq!(identities.len(), 7 * 8 * (2 * 256 + 2));
    assert_eq!(identities.len(), 28_784);
}

#[test]
fn all_fifty_six_address_source_pairs_have_only_owned_rmw_admission() {
    let mut modules = Vec::new();
    let mut pairs = BTreeSet::new();
    for (shape_index, shape) in shapes().iter().enumerate() {
        for source in 0..8 {
            assert!(pairs.insert((shape_index, source)));
            let bytes = bank(shape, source);
            let engine = code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            let original = page(memory, CODE);
            let expected = error(CODE, InstructionError::BackendUnsupported);
            assert_eq!(
                prepare_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(
                prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(page(memory, CODE), original);
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes, true);
                    describe(&mut engine, &[(CODE, bytes.len())], entries);
                    let before = engine.arena().to_vec();
                    let before_code = page(engine.memory().unwrap(), CODE);
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_eq!(page(engine.memory().unwrap(), CODE), before_code);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(DATA), Access::Read)
                            .is_err()
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(CODE), Access::Read)
                            .is_err()
                    );
                    assert_rmw_module(&engine, owner, id);
                    guard(&engine, owner, id).unwrap();
                    assert_eq!(
                        engine.generation(),
                        if owner == Owner::Replacement {
                            id as u32
                        } else {
                            0
                        }
                    );
                    if owner == Owner::Resident {
                        let mut offset = 0;
                        for (_, opcode) in FORMS {
                            assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                            offset += instruction(
                                opcode,
                                shape,
                                source,
                                if opcode == 0xa4 { 0 } else { 32 },
                            )
                            .len() as u32;
                        }
                        assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                        assert!(engine.lookup_resident(CODE + bytes.len() as u32).is_err());
                    }
                    modules.push((owner, module(&engine, owner, id).to_vec()));
                }
            }
        }
    }
    assert_eq!(pairs.len(), 56);
    assert_eq!(modules.len(), 56 * 4);
    assert_eq!(modules.len(), 224);
    validate_modules(&modules);
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
    mapped_pages: u32,
    pages: [Vec<u8>; 2],
}

fn saved(engine: &EngineInstance, keep: u64) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(keep).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        mapped_pages: engine.memory().unwrap().mapped_pages(),
        pages: [CODE, KEEP].map(|pc| page(engine.memory().unwrap(), pc)),
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, &[(KEEP, 3)], false);
    engine.compile(1).unwrap();
    describe(&mut engine, &[(KEEP, 3)], false);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn assert_retained(engine: &EngineInstance, keep: u64, before: &Saved) {
    assert_eq!(&saved(engine, keep), before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.lookup_resident(CODE).is_err());
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let mut cases = Vec::new();
    let shape = &shapes()[1];
    for (_, opcode) in FORMS {
        let memory = instruction(opcode, shape, 2, 1);
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
        ] {
            let expected = match prefix {
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                0xf0 => DecodeError::InvalidEncoding,
                _ => DecodeError::Unsupported(UnsupportedFeature::Opcode),
            };
            let mut bytes = vec![prefix];
            bytes.extend_from_slice(&memory);
            cases.push((bytes, expected));
        }
        let mut word_register = vec![0x66, 0x0f, opcode, 0xd0];
        if matches!(opcode, 0xa4 | 0xac) {
            word_register.push(1);
        }
        cases.push((
            word_register,
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ));
    }
    assert_eq!(cases.len(), 48);
    assert_eq!(
        cases
            .iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        48
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, error)| *error == DecodeError::InvalidEncoding)
            .count(),
        4
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, error)| *error == DecodeError::Unsupported(UnsupportedFeature::Segment))
            .count(),
        24
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, error)| *error == DecodeError::Unsupported(UnsupportedFeature::Opcode))
            .count(),
        20
    );
    cases
}

#[test]
fn closed_prefix_profiles_and_split_seeds_preserve_both_prior_publications() {
    let positive = instruction(0xa4, &shapes()[1], 2, 1);
    for (excluded, cause) in exclusions() {
        let mut bytes = positive.clone();
        bytes.extend_from_slice(&excluded);
        let failed_pc = CODE + positive.len() as u32;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &bytes);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let before = saved(&engine, keep);
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(failed_pc)).err(),
                    Some(cause)
                );
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(
                        owner,
                        error(failed_pc, InstructionError::Decode(cause))
                    ))
                );
                assert_retained(&engine, keep, &before);
            }
        }
    }
    for (_, opcode) in FORMS {
        let target = instruction(opcode, &shapes()[6], 1, 32);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                let mut bytes = target.clone();
                bytes.extend([0xeb, 0]);
                upload(&mut engine, CODE, &bytes);
                let specs = if entries {
                    vec![(CODE, 0), (CODE + target.len() as u32 - 1, 0)]
                } else {
                    vec![(CODE, target.len() - 1)]
                };
                describe(&mut engine, &specs, entries);
                let before = saved(&engine, keep);
                let expected = if entries {
                    CompileError::InvalidBlocks
                } else {
                    error(CODE, InstructionError::InvalidBlockEnd)
                };
                assert_eq!(
                    compile(&mut engine, owner, entries, specs.len() as u32),
                    Err(compile_error(owner, expected))
                );
                assert_retained(&engine, keep, &before);
            }
        }
    }
}

#[test]
fn exact_fetch_cuts_page_boundaries_and_top_wrap_are_not_operand_reads() {
    for shape in shapes() {
        for (_, opcode) in FORMS {
            let target = instruction(opcode, &shape, 1, 255);
            let length = target.len() as u32;
            for pc in [0x2000 - length, u32::MAX - length + 1] {
                let engine = code(pc, &target, true);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (length as u8, GuestAddress(pc.wrapping_add(length)))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                if pc >= 0xffff_f000 {
                    for owner in [Owner::Replacement, Owner::Resident] {
                        for entries in [false, true] {
                            let mut engine = code(pc, &target, true);
                            describe(&mut engine, &[(pc, target.len())], entries);
                            let before = engine.arena().to_vec();
                            let id = compile(&mut engine, owner, entries, 1).unwrap();
                            assert_eq!(engine.arena(), before);
                            assert_rmw_module(&engine, owner, id);
                        }
                    }
                }
            }
            for present in 1..target.len() {
                for top in [false, true] {
                    let pc = if top {
                        u32::MAX - present as u32 + 1
                    } else {
                        0x2000 - present as u32
                    };
                    let engine = code(pc, &target[..present], true);
                    let expected = fetch_error(
                        pc,
                        if top { pc } else { 0x2000 },
                        present as u32 + 1,
                        if top {
                            FaultReason::AddressOverflow
                        } else {
                            FaultReason::Unmapped
                        },
                    );
                    assert_eq!(
                        decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                        Some(expected)
                    );
                }
            }
            let pc = 0x2000 - length + 1;
            let mut engine = code(pc, &target, false);
            engine.protect(0x2000, 1, 1).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, length, FaultReason::Permission))
            );
            let mut bytes = target.clone();
            bytes.extend([0xeb, 0]);
            let pc = 0x2000 - bytes.len() as u32;
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes, true);
                    describe(&mut engine, &[(pc, bytes.len())], entries);
                    let before = engine.arena().to_vec();
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_rmw_module(&engine, owner, id);
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(DATA), Access::Read)
                            .is_err()
                    );
                }
            }
        }
    }
    for opcode in [0xa4, 0xac] {
        let target = instruction(opcode, &shapes()[6], 1, 32);
        let missing = &target[..target.len() - 1];
        let pc = 0x2000 - missing.len() as u32;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, pc, missing);
                engine.protect(CODE, 1, 4).unwrap();
                describe(&mut engine, &[(pc, target.len())], entries);
                let before = saved(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(
                        owner,
                        error(
                            pc,
                            InstructionError::Decode(fetch_error(
                                pc,
                                0x2000,
                                target.len() as u32,
                                FaultReason::Unmapped
                            ))
                        )
                    ))
                );
                assert_retained(&engine, keep, &before);
            }
        }
        let mut crossing = target.clone();
        crossing.extend([0xeb, 0]);
        let pc = 0x2000 - target.len() as u32 + 1;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(pc, &crossing, true);
                describe(&mut engine, &[(pc, crossing.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_rmw_module(&engine, owner, id);
                guard(&engine, owner, id).unwrap();
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
            }
        }
    }
}

#[test]
fn each_consumed_byte_versions_same_and_changed_writes_without_data_currency() {
    for (_, opcode) in FORMS {
        let target = instruction(opcode, &shapes()[6], 1, 32);
        let mut bytes = target.clone();
        bytes.extend([0xeb, 0]);
        let pc = 0x1ffc;
        for offset in 0..target.len() {
            for same in [true, false] {
                for owner in [Owner::Replacement, Owner::Resident] {
                    for entries in [false, true] {
                        let mut engine = code(pc, &bytes, false);
                        let snapshot = engine
                            .memory()
                            .unwrap()
                            .snapshot_code(GuestAddress(pc), target.len())
                            .unwrap();
                        describe(&mut engine, &[(pc, bytes.len())], entries);
                        let id = compile(&mut engine, owner, entries, 1).unwrap();
                        let before = module(&engine, owner, id).to_vec();
                        let pointer = module(&engine, owner, id).as_ptr();
                        engine.map(DATA, 1, 3).unwrap();
                        engine.write32(DATA + 0x10, 0xffff_fff7).unwrap();
                        engine.protect(DATA, 1, 1).unwrap();
                        engine.unmap(DATA, 1).unwrap();
                        assert_eq!(module(&engine, owner, id), before);
                        assert_eq!(module(&engine, owner, id).as_ptr(), pointer);
                        assert!(engine.memory().unwrap().is_code_current(&snapshot));
                        guard(&engine, owner, id).unwrap();
                        let changed = if same {
                            target[offset]
                        } else {
                            target[offset] ^ 1
                        };
                        upload(&mut engine, pc + offset as u32, &[changed]);
                        let arena = engine.arena().to_vec();
                        assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                        let expected = if owner == Owner::Replacement {
                            HostError::CodeInvalidated
                        } else {
                            HostError::Resident(RegistryError::CodeInvalidated)
                        };
                        assert_eq!(guard(&engine, owner, id), Err(expected));
                        match owner {
                            Owner::Replacement => {
                                assert_eq!(engine.artifact_bytes(), Err(expected))
                            }
                            Owner::Resident => assert_eq!(engine.resident_bytes(id), Err(expected)),
                        }
                        assert_eq!(engine.arena(), arena);
                        assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                    }
                }
            }
        }
    }
}

#[test]
fn instruction_caps_and_pure_tiny_wasm_fail_without_replacing_owners() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for (_, opcode) in FORMS {
        let target = instruction(opcode, &shapes()[1], 1, 32);
        let mut admitted = vec![0x90; 62];
        admitted.extend_from_slice(&target);
        admitted.extend([0xeb, 0]);
        let mut rejected = vec![0x90; 63];
        rejected.extend_from_slice(&target);
        rejected.extend([0x0f, 0x0b]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &admitted);
                describe(&mut engine, &[(CODE, admitted.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_rmw_module(&engine, owner, id);
                guard(&engine, owner, id).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &rejected);
                describe(&mut engine, &[(CODE, rejected.len())], entries);
                let before = saved(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(owner, CompileError::InstructionLimit))
                );
                assert_retained(&engine, keep, &before);
            }
        }
    }
    let (mut engine, keep) = prior_owners();
    let pure = [0x90, 0xeb, 0];
    upload(&mut engine, CODE, &pure);
    let before = saved(&engine, keep);
    let tiny = CompileLimits {
        wasm_bytes: 1,
        ..limits
    };
    let memory = engine.memory().unwrap();
    assert_eq!(
        compile_region(memory, &[spec(CODE, pure.len())], tiny).err(),
        Some(CompileError::WasmLimit)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], tiny).err(),
        Some(CompileError::WasmLimit)
    );
    assert_retained(&engine, keep, &before);
}
