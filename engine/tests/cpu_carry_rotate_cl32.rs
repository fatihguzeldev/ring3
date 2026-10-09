use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn admits_register_carry_cl(bytes: &[u8; 4]) {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    let compiled = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: 4,
        }],
        CompileLimits::default(),
    )
    .expect("register carry rotate by CL must compile");
    assert_eq!(compiled.metadata().blocks, 1);
    assert_eq!(compiled.metadata().instructions, 2);
}

#[test]
fn register_rcl_cl_admits_before_jump() {
    admits_register_carry_cl(&[0xd3, 0xd0, 0xeb, 0]);
}

#[test]
fn register_rcr_ecx_cl_admits_before_jump() {
    admits_register_carry_cl(&[0xd3, 0xd9, 0xeb, 0]);
}

use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{EFLAGS_OFFSET, REGISTERS_OFFSET},
    },
    cpu::{
        UnsupportedFeature,
        dbt::{ArtifactError, CompileError, InstructionError, RegistryError, compile_entry_region},
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Operation, RotateKind},
        },
    },
    memory::{Access, FaultReason, MemoryFault},
    process::{EngineInstance, HostError},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [RotateKind; 2] = [RotateKind::Left, RotateKind::Right];
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

fn instruction(kind: RotateKind, destination: Register32) -> [u8; 2] {
    [
        0xd3,
        0xd0 | (u8::from(kind == RotateKind::Right) << 3) | destination.index() as u8,
    ]
}

fn expected(kind: RotateKind, destination: Register32) -> Operation {
    Operation::RotateThroughCarryCl { kind, destination }
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

fn describe(engine: &mut EngineInstance, specs: &[BlockSpec], entries: bool) {
    for (index, item) in specs.iter().enumerate() {
        let at = TRANSFER_OFFSET + index * if entries { 4 } else { 8 };
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&item.entry.0.to_le_bytes());
        if !entries {
            engine.arena_mut().unwrap()[at + 4..at + 8]
                .copy_from_slice(&item.byte_length.to_le_bytes());
        }
    }
}

fn compile(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
    count: u32,
) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(count).map(u64::from),
        (false, true) => engine.compile_entries(count, 0).map(u64::from),
        (true, false) => engine.compile_resident(count).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    }
}

fn error(pc: u32, cause: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(cause),
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

fn page(engine: &EngineInstance, pc: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(pc), &mut bytes)
        .unwrap();
    bytes
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    key: u64,
    modules: [(Vec<u8>, usize); 2],
    pages: Vec<(u32, Vec<u8>)>,
    mapped: u32,
    resident_at_keep: u64,
}

fn old_owners(engine: &mut EngineInstance) -> u64 {
    engine.map(KEEP, 1, 7).unwrap();
    upload(engine, KEEP, &[0x90, 0xeb, 0]);
    describe(engine, &[spec(KEEP, 3)], false);
    engine.compile(1).unwrap();
    engine.compile_resident(1).unwrap().get()
}

fn saved(engine: &EngineInstance, resident: u64, code_pages: &[u32]) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        key: engine.key(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(resident).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: code_pages
            .iter()
            .copied()
            .chain([KEEP])
            .map(|pc| (pc, page(engine, pc)))
            .collect(),
        mapped: engine.memory().unwrap().mapped_pages(),
        resident_at_keep: engine.lookup_resident(KEEP).unwrap().get(),
    }
}

fn retained(engine: &EngineInstance, resident: u64, code_pages: &[u32], before: &Saved) {
    assert_eq!(&saved(engine, resident, code_pages), before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, resident).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), resident);
}

fn validate_modules(modules: &[(u8, Vec<u8>)]) {
    let mut input = Vec::new();
    for (owner, bytes) in modules {
        input.push(*owner);
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
    }
    // inspect native-generated modules without instantiating or running guest code.
    let script = r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let offset = 0, count = 0;
while (offset < input.length) {
  assert.ok(offset + 5 <= input.length);
  const owner = input[offset], length = input.readUInt32LE(offset + 1);
  assert.ok(owner <= 2); offset += 5;
  assert.ok(length > 8 && offset + length <= input.length);
  const bytes = input.subarray(offset, offset + length); offset += length;
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes);
  const imports = [{module:'env', name:'memory', kind:'memory'}];
  if (owner) imports.push({module:'ring3', name:owner === 1 ? 'guard' : 'guard_resident', kind:'function'});
  assert.deepEqual(WebAssembly.Module.imports(module), imports);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run', kind:'function'}]);
  let at = 8;
  const u = () => {
    let value = 0, shift = 0;
    for (let i = 0; i < 5; i++) {
      assert.ok(at < bytes.length);
      const byte = bytes[at++]; value |= (byte & 127) << shift;
      if (!(byte & 128)) return value >>> 0;
      shift += 7;
    }
    assert.fail('invalid bounded unsigned LEB');
  };
  const vector = () => Array.from({length:u()}, () => bytes[at++]);
  const text = () => { const length = u(), start = at; at += length; return bytes.subarray(start, at).toString('utf8'); };
  const seen = new Set();
  while (at < bytes.length) {
    const id = bytes[at++], length = u(), end = at + length;
    assert.ok(end <= bytes.length);
    if ([1,3,7,10].includes(id)) assert.ok(!seen.has(id));
    seen.add(id);
    if (id === 1) {
      const types = [];
      for (let n = u(); n > 0; n--) { assert.equal(bytes[at++], 0x60); types.push([vector(), vector()]); }
      const expected = [[Array(4).fill(0x7f), [0x7f]]];
      if (owner) expected.push([Array(owner === 1 ? 6 : 7).fill(0x7f), [0x7f]]);
      assert.deepEqual(types, expected);
    } else if (id === 3) {
      assert.equal(u(), 1); assert.equal(u(), 0);
    } else if (id === 7) {
      assert.equal(u(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(u(), owner ? 1 : 0);
    } else if (id === 10) {
      assert.equal(u(), 1); const bodyLength = u(), bodyEnd = at + bodyLength, locals = [];
      for (let n = u(); n > 0; n--) locals.push([u(), bytes[at++]]);
      assert.deepEqual(locals, [[16,0x7f],[1,0x7e]]);
      assert.equal(bodyEnd, end); at = bodyEnd;
    } else at = end;
    assert.equal(at, end);
  }
  for (const id of [1,3,7,10]) assert.ok(seen.has(id));
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
fn all_register_cl_decode_identities_preserve_old_one_and_immediate_neighbors() {
    let mut engine = code(CODE, &[0x90], false);
    let mut decoded_cl = 0;
    let mut neighbors = 0;
    for kind in KINDS {
        for destination in REGISTERS {
            upload(&mut engine, CODE, &instruction(kind, destination));
            let memory = engine.memory().unwrap();
            let decoded = decode_one(memory, GuestAddress(CODE)).unwrap();
            assert_eq!(decoded.operation(), &expected(kind, destination));
            assert_eq!(
                (decoded.pc(), decoded.length(), decoded.next_pc()),
                (GuestAddress(CODE), 2, GuestAddress(CODE + 2))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            decoded_cl += 1;
        }
        for destination in [Register32::Eax, Register32::Ecx] {
            let modrm = instruction(kind, destination)[1];
            upload(&mut engine, CODE, &[0xd1, modrm]);
            let memory = engine.memory().unwrap();
            let decoded = decode_one(memory, GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::RotateThroughCarryOne { kind, destination }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (2, GuestAddress(CODE + 2))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            neighbors += 1;
            for raw in [0, 1, 2, 31, 32, 33, 255] {
                upload(&mut engine, CODE, &[0xc1, modrm, raw]);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(CODE)).unwrap();
                let operation = if raw & 31 == 1 {
                    Operation::RotateThroughCarryOne { kind, destination }
                } else {
                    Operation::RotateThroughCarryImmediate {
                        kind,
                        destination,
                        count: raw,
                    }
                };
                assert_eq!(decoded.operation(), &operation);
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (3, GuestAddress(CODE + 3))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                neighbors += 1;
            }
        }
    }
    assert_eq!((decoded_cl, neighbors), (16, 32));
}

#[test]
fn all_register_banks_validate_six_profiles_without_memory_helpers() {
    let mut modules = Vec::new();
    for kind in KINDS {
        let mut bytes = Vec::new();
        for destination in REGISTERS {
            bytes.extend(instruction(kind, destination));
        }
        bytes.extend([0xeb, 0]);
        assert_eq!(bytes.len(), 18);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        for artifact in [
            compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default()).unwrap(),
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
        ] {
            assert_eq!(
                (artifact.metadata().blocks, artifact.metadata().instructions),
                (1, 9)
            );
            modules.push((0, artifact.wasm_bytes(memory).unwrap().to_vec()));
        }
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, &[spec(CODE, bytes.len())], entries);
                let arena = engine.arena().to_vec();
                let id = compile(&mut engine, resident, entries, 1).unwrap();
                let module = if resident {
                    engine.guard_resident(KEY, id).unwrap();
                    for pc in (0..8).map(|index| CODE + index * 2).chain([CODE + 16]) {
                        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                    }
                    for index in 0..8 {
                        assert!(engine.lookup_resident(CODE + index * 2 + 1).is_err());
                    }
                    assert!(engine.lookup_resident(CODE + 17).is_err());
                    assert!(engine.lookup_resident(CODE + 18).is_err());
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.guard(KEY, id as u32).unwrap();
                    engine.artifact_bytes().unwrap()
                };
                assert!(module.len() <= CompileLimits::default().wasm_bytes);
                modules.push((if resident { 2 } else { 1 }, module.to_vec()));
                assert_eq!(engine.arena(), arena);
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
    }
    assert_eq!(modules.len(), 12);
    validate_modules(&modules);
}

fn memory_instruction(kind: RotateKind, absolute: bool, opcode: u8, raw: u8) -> Vec<u8> {
    let mut bytes = vec![
        opcode,
        0x10 | (u8::from(kind == RotateKind::Right) << 3) | if absolute { 5 } else { 3 },
    ];
    if absolute {
        bytes.extend(DATA.to_le_bytes());
    }
    if opcode == 0xc1 {
        bytes.push(raw);
    }
    bytes
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut rows = Vec::new();
    for kind in KINDS {
        for destination in [Register32::Eax, Register32::Ecx] {
            for prefix in [
                0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
            ] {
                let failure = match prefix {
                    0xf0 => DecodeError::InvalidEncoding,
                    0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                        DecodeError::Unsupported(UnsupportedFeature::Segment)
                    }
                    _ => opcode,
                };
                let mut bytes = vec![prefix];
                if prefix == 0x66 {
                    bytes.push(0x66);
                }
                bytes.extend(instruction(kind, destination));
                rows.push((bytes, failure));
            }
        }
        for absolute in [false, true] {
            rows.push((
                [vec![0x66], memory_instruction(kind, absolute, 0xd3, 0)].concat(),
                opcode,
            ));
            for raw in [0, 2, 31, 32, 255] {
                rows.push((
                    [vec![0x66], memory_instruction(kind, absolute, 0xc1, raw)].concat(),
                    opcode,
                ));
            }
        }
    }
    for operand in [0xf0, 0x33] {
        rows.push((vec![0xd3, operand], opcode));
        rows.push((vec![0xc1, operand, 2], opcode));
    }
    assert_eq!(rows.len(), 72);
    assert_eq!(
        rows.iter()
            .filter(|(_, failure)| *failure == DecodeError::InvalidEncoding)
            .count(),
        4
    );
    assert_eq!(rows.iter().filter(|(_, failure)| *failure == DecodeError::Unsupported(UnsupportedFeature::Segment)).count(), 24);
    assert_eq!(
        rows.iter()
            .filter(|(_, failure)| *failure == opcode)
            .count(),
        44
    );
    rows
}

fn failed_compile_profiles(pc: u32, bytes: &[u8], declared: usize, failure: CompileError) -> usize {
    let mut checks = 0;
    let engine = code(pc, bytes, false);
    let memory = engine.memory().unwrap();
    let source = page(&engine, pc & !0xfff);
    assert_eq!(
        compile_region(memory, &[spec(pc, declared)], CompileLimits::default()).err(),
        Some(failure)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()).err(),
        Some(failure)
    );
    assert_eq!(page(&engine, pc & !0xfff), source);
    checks += 2;
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(pc, bytes, false);
            let keep = old_owners(&mut engine);
            describe(&mut engine, &[spec(pc, declared)], entries);
            let pages = [pc & !0xfff];
            let before = saved(&engine, keep, &pages);
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(failure))
            } else {
                HostError::Compile(failure)
            };
            assert_eq!(compile(&mut engine, resident, entries, 1), Err(expected));
            retained(&engine, keep, &pages, &before);
            assert!(engine.lookup_resident(pc).is_err());
            checks += 1;
        }
    }
    checks
}

#[test]
fn strict_and_late_failures_keep_both_published_owners() {
    for (bytes, expected) in exclusions() {
        let engine = code(CODE, &bytes, false);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    }
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut strict = 0;
    for (excluded, expected) in [
        (vec![0x66, 0x66, 0xd3, 0xd0], opcode),
        (
            vec![0x64, 0xd3, 0xd0],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
        (vec![0xf0, 0xd3, 0xd0], DecodeError::InvalidEncoding),
        (vec![0x66, 0xc1, 0x13, 2], opcode),
        (vec![0x66, 0xd3, 0x13], opcode),
        (vec![0xd3, 0xf0], opcode),
    ] {
        let mut bytes = instruction(RotateKind::Left, Register32::Eax).to_vec();
        bytes.extend(excluded);
        strict += failed_compile_profiles(CODE, &bytes, bytes.len(), error(CODE + 2, expected));
    }
    let mut late = 0;
    for kind in KINDS {
        let target = instruction(kind, Register32::Eax);
        let mut unsupported = target.to_vec();
        unsupported.extend([0x0f, 0x0b]);
        late += failed_compile_profiles(
            CODE,
            &unsupported,
            unsupported.len(),
            error(CODE + 2, opcode),
        );
        let mut prefixed = target.to_vec();
        prefixed.extend([0x66, 0x66]);
        prefixed.extend(instruction(kind, Register32::Ecx));
        late += failed_compile_profiles(CODE, &prefixed, prefixed.len(), error(CODE + 2, opcode));
        let mut truncated = target.to_vec();
        truncated.push(0xd3);
        late += failed_compile_profiles(
            0x1ffd,
            &truncated,
            4,
            error(
                0x1fff,
                fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
            ),
        );
    }
    assert_eq!((strict, late), (36, 36));
}

#[test]
fn exact_two_byte_fetch_preserves_page_top_and_permission_boundaries() {
    let mut complete = 0;
    let mut truncated = 0;
    let mut permissions = 0;
    for kind in KINDS {
        for destination in [Register32::Eax, Register32::Esp] {
            let bytes = instruction(kind, destination);
            for pc in [CODE, 0x1ffe, u32::MAX - 1, 0x1fff] {
                let engine = code(pc, &bytes, true);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(decoded.operation(), &expected(kind, destination));
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (2, GuestAddress(pc.wrapping_add(2)))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                if pc == 0x1ffe {
                    assert!(
                        memory
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                }
                complete += 1;
            }
            for top in [false, true] {
                let pc = if top { u32::MAX } else { 0x1fff };
                let engine = code(pc, &bytes[..1], true);
                let expected = fetch_error(
                    pc,
                    if top { pc } else { 0x2000 },
                    2,
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
                truncated += 1;
            }
            let mut engine = code(CODE, &bytes, false);
            engine.protect(CODE, 1, 1).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
            );
            permissions += 1;
            let mut engine = code(0x1fff, &bytes, false);
            engine.protect(0x2000, 1, 1).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).err(),
                Some(fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission))
            );
            permissions += 1;
        }
    }
    assert_eq!((complete, truncated, permissions), (16, 8, 8));
}

#[test]
fn runtime_cpu_changes_keep_currency_but_consumed_bytes_invalidate_owners() {
    let mut changes = 0;
    let mut cpu_inputs = 0;
    for kind in KINDS {
        let target = instruction(kind, Register32::Ecx);
        for changed in 0..2 {
            for same in [false, true] {
                let pc = 0x1fff;
                let mut bytes = target.to_vec();
                bytes.extend([0xeb, 0]);
                let mut engine = code(pc, &bytes, false);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                let memory = engine.memory().unwrap();
                let pure = [
                    compile_region(memory, &[spec(pc, bytes.len())], CompileLimits::default())
                        .unwrap(),
                    compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                        .unwrap(),
                ];
                let pure_bytes: Vec<_> = pure
                    .iter()
                    .map(|artifact| {
                        let bytes = artifact.wasm_bytes(memory).unwrap();
                        (bytes.to_vec(), bytes.as_ptr() as usize)
                    })
                    .collect();
                describe(&mut engine, &[spec(pc, bytes.len())], false);
                let generation = engine.compile(1).unwrap();
                let resident = engine.compile_resident(1).unwrap().get();
                let bound_bytes = [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(resident).unwrap(),
                ]
                .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
                let identity = (
                    engine.key(),
                    engine.generation(),
                    engine.arena_address(),
                    engine.memory().unwrap().mapped_pages(),
                );
                let source = [page(&engine, 0x1000), page(&engine, 0x2000)];
                let assert_current = |engine: &EngineInstance| {
                    assert_eq!(
                        (
                            engine.key(),
                            engine.generation(),
                            engine.arena_address(),
                            engine.memory().unwrap().mapped_pages()
                        ),
                        identity
                    );
                    assert_eq!([page(engine, 0x1000), page(engine, 0x2000)], source);
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    for (artifact, expected) in pure.iter().zip(&pure_bytes) {
                        let bytes = artifact.wasm_bytes(engine.memory().unwrap()).unwrap();
                        assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), expected);
                    }
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, resident).unwrap();
                    assert_eq!(
                        [
                            engine.artifact_bytes().unwrap(),
                            engine.resident_bytes(resident).unwrap()
                        ]
                        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
                        bound_bytes
                    );
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), resident);
                };
                engine.map(DATA, 1, 3).unwrap();
                engine.write32(DATA, 0x9234_5678).unwrap();
                engine.protect(DATA, 1, 1).unwrap();
                engine.unmap(DATA, 1).unwrap();
                assert_current(&engine);
                for ecx in [0u32, 1, 2, 31, 32, 33, 255, 0x8000_0002] {
                    for flags in [2u32, 0xcd7] {
                        let ecx_at = REGISTERS_OFFSET + Register32::Ecx.index() * 4;
                        let mut expected_arena = engine.arena().to_vec();
                        expected_arena[ecx_at..ecx_at + 4].copy_from_slice(&ecx.to_le_bytes());
                        expected_arena[EFLAGS_OFFSET..EFLAGS_OFFSET + 4]
                            .copy_from_slice(&flags.to_le_bytes());
                        engine.arena_mut().unwrap()[ecx_at..ecx_at + 4]
                            .copy_from_slice(&ecx.to_le_bytes());
                        engine.arena_mut().unwrap()[EFLAGS_OFFSET..EFLAGS_OFFSET + 4]
                            .copy_from_slice(&flags.to_le_bytes());
                        assert_eq!(engine.arena(), expected_arena);
                        assert_current(&engine);
                        cpu_inputs += 1;
                    }
                }
                let replacement = if same {
                    target[changed]
                } else {
                    match changed {
                        0 => 0xd1,
                        1 => target[1] ^ 1,
                        _ => unreachable!(),
                    }
                };
                assert_eq!(replacement == target[changed], same);
                upload(&mut engine, pc + changed as u32, &[replacement]);
                assert!(
                    !engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                for artifact in &pure {
                    assert_eq!(
                        artifact.wasm_bytes(engine.memory().unwrap()),
                        Err(ArtifactError::CodeInvalidated)
                    );
                }
                assert_eq!(
                    engine.guard(KEY, generation),
                    Err(HostError::CodeInvalidated)
                );
                assert_eq!(
                    engine.guard_resident(KEY, resident),
                    Err(HostError::Resident(RegistryError::CodeInvalidated))
                );
                changes += 1;
            }
        }
    }
    assert_eq!((changes, cpu_inputs), (8, 128));
}

#[test]
fn default_instruction_block_and_tight_wasm_caps_retain_prior_owners() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut positive = 0;
    let mut negative = 0;
    for kind in KINDS {
        let target = instruction(kind, Register32::Ecx);
        let mut bytes = vec![0x90; 62];
        bytes.extend(target);
        bytes.extend([0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        for artifact in [
            compile_region(memory, &[spec(CODE, bytes.len())], limits).unwrap(),
            compile_entry_region(memory, &[GuestAddress(CODE)], limits).unwrap(),
        ] {
            assert_eq!(artifact.metadata().instructions, 64);
            positive += 1;
        }
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, &[spec(CODE, bytes.len())], entries);
                let before = engine.arena().to_vec();
                compile(&mut engine, resident, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                positive += 1;
            }
        }
        let mut bytes = vec![0x90; 63];
        bytes.extend(target);
        bytes.extend([0x0f, 0x0b]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        assert_eq!(
            compile_region(memory, &[spec(CODE, bytes.len())], limits).err(),
            Some(CompileError::InstructionLimit)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
            Some(CompileError::InstructionLimit)
        );
        negative += 2;
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, false);
                let keep = old_owners(&mut engine);
                describe(&mut engine, &[spec(CODE, bytes.len())], entries);
                let before = saved(&engine, keep, &[CODE]);
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
                } else {
                    HostError::Compile(CompileError::InstructionLimit)
                };
                assert_eq!(compile(&mut engine, resident, entries, 1), Err(expected));
                retained(&engine, keep, &[CODE], &before);
                negative += 1;
            }
        }
    }
    assert_eq!((positive, negative), (12, 12));
    let atom = [0xd3, 0xd0, 0xeb, 0];
    let bytes = atom.repeat(9);
    let specs: Vec<_> = (0..9).map(|index| spec(CODE + index * 4, 4)).collect();
    let starts: Vec<_> = specs.iter().map(|spec| spec.entry).collect();
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    for artifact in [
        compile_region(memory, &specs[..8], limits).unwrap(),
        compile_entry_region(memory, &starts[..8], limits).unwrap(),
    ] {
        assert_eq!(
            (artifact.metadata().blocks, artifact.metadata().instructions),
            (8, 16)
        );
    }
    assert_eq!(
        compile_region(memory, &specs, limits).err(),
        Some(CompileError::InvalidBlocks)
    );
    assert_eq!(
        compile_entry_region(memory, &starts, limits).err(),
        Some(CompileError::InvalidBlocks)
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, false);
            describe(&mut engine, &specs[..8], entries);
            compile(&mut engine, resident, entries, 8).unwrap();
            let mut engine = code(CODE, &bytes, false);
            let keep = old_owners(&mut engine);
            describe(&mut engine, &specs, entries);
            let before = saved(&engine, keep, &[CODE]);
            assert_eq!(
                compile(&mut engine, resident, entries, 9),
                Err(HostError::InvalidRequest)
            );
            retained(&engine, keep, &[CODE], &before);
        }
    }
    let mut bytes = Vec::new();
    for destination in REGISTERS {
        bytes.extend(instruction(RotateKind::Right, destination));
    }
    bytes.extend([0xeb, 0]);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let size = compile_region(memory, &[spec(CODE, bytes.len())], limits)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap()
        .len();
    for tight in [size, size - 1] {
        let limits = CompileLimits {
            wasm_bytes: tight,
            ..limits
        };
        for compiled in [
            compile_region(memory, &[spec(CODE, bytes.len())], limits),
            compile_entry_region(memory, &[GuestAddress(CODE)], limits),
        ] {
            if tight == size {
                assert!(compiled.is_ok());
            } else {
                assert_eq!(compiled.err(), Some(CompileError::WasmLimit));
            }
        }
    }
}
