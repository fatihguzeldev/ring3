use ring3_engine::process::EngineInstance;

const KEY: u64 = 0x1234_5678_9abc_def0;

#[test]
fn register_rol32_immediate_two_admits_before_jump() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let code = [0xc1, 0xc0, 0x02, 0xeb, 0x00];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
    engine.upload(0x1000, 5).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&5_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register ROL32 with immediate count two must admit before a jump");
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
            ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation, RotateKind, ShiftCount},
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
const KINDS: [RotateKind; 2] = [RotateKind::Left, RotateKind::Right];
const COUNTS: [u8; 9] = [0, 1, 2, 31, 32, 33, 64, 128, 255];

fn instruction(opcode: u8, kind: RotateKind, destination: u8, raw: u8) -> Vec<u8> {
    let mut bytes = vec![
        opcode,
        0xc0 | u8::from(kind == RotateKind::Right) << 3 | destination,
    ];
    if opcode == 0xc1 {
        bytes.push(raw);
    }
    bytes
}

fn expected(kind: RotateKind, destination: Register32, opcode: u8, raw: u8) -> Operation {
    if opcode == 0xd1 || (opcode == 0xc1 && raw & 31 == 1) {
        Operation::RotateOne { kind, destination }
    } else {
        Operation::Rotate {
            kind,
            destination,
            count: if opcode == 0xd3 {
                ShiftCount::Cl
            } else {
                ShiftCount::Immediate(raw)
            },
        }
    }
}

fn compact_bank(kind: RotateKind, destination: u8) -> (Vec<u8>, Vec<usize>) {
    let mut bytes = Vec::new();
    let mut starts = vec![0];
    bytes.extend(instruction(0xd1, kind, destination, 0));
    for raw in COUNTS {
        starts.push(bytes.len());
        bytes.extend(instruction(0xc1, kind, destination, raw));
    }
    starts.push(bytes.len());
    bytes.extend(instruction(0xd3, kind, destination, 0));
    starts.push(bytes.len());
    bytes.extend([0xeb, 0]);
    (bytes, starts)
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
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

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    transfer[4..].copy_from_slice(&if entries { 0 } else { length as u32 }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn page(memory: &AddressSpace, pc: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    memory.fetch(GuestAddress(pc), &mut bytes).unwrap();
    bytes
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

fn validate_modules(modules: &[(u8, Vec<u8>)]) {
    let mut input = Vec::new();
    for (owner, bytes) in modules {
        input.push(*owner);
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
    }
    // validate and inspect compiled modules without instantiating or running guest code.
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
fn all_register_raw_counts_and_old_one_forms_preserve_exact_decode_identity() {
    let mut engine = code(CODE, &[0x90; 3], false);
    let mut identities = BTreeSet::new();
    let (mut old_immediate, mut new_immediate, mut cl, mut implicit) = (0, 0, 0, 0);
    for kind in KINDS {
        for (index, destination) in REGISTERS.into_iter().enumerate() {
            for opcode in [0xc1, 0xd3, 0xd1] {
                let counts: Vec<u8> = if opcode == 0xc1 {
                    (0..=u8::MAX).collect()
                } else {
                    vec![0]
                };
                for raw in counts {
                    let bytes = instruction(opcode, kind, index as u8, raw);
                    assert!(identities.insert(bytes.clone()));
                    upload(&mut engine, CODE, &bytes);
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                    assert_eq!(
                        decoded.operation(),
                        &expected(kind, destination, opcode, raw)
                    );
                    assert_eq!(
                        (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
                        (
                            GuestAddress(CODE),
                            bytes.len(),
                            GuestAddress(CODE + bytes.len() as u32)
                        )
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    match opcode {
                        0xc1 if raw & 31 == 1 => old_immediate += 1,
                        0xc1 => new_immediate += 1,
                        0xd3 => cl += 1,
                        0xd1 => implicit += 1,
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
    assert_eq!(
        (old_immediate, new_immediate, cl, implicit),
        (128, 3968, 16, 16)
    );
    assert_eq!(old_immediate + new_immediate + cl, 4112);
    assert_eq!(new_immediate + cl, 3984);
    assert_eq!(identities.len(), 4128);
}

#[test]
fn sixteen_compact_banks_admit_six_profiles_with_exact_pure_wasm_abi() {
    let mut modules = Vec::new();
    for kind in KINDS {
        for destination in 0..8 {
            let (bytes, starts) = compact_bank(kind, destination);
            assert_eq!(bytes.len(), 33);
            assert_eq!(starts, [0, 2, 5, 8, 11, 14, 17, 20, 23, 26, 29, 31]);
            let engine = code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            let before = page(memory, CODE);
            let explicit =
                compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default())
                    .unwrap();
            let entries =
                compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .unwrap();
            assert_eq!(explicit.metadata(), entries.metadata());
            assert_eq!(
                explicit.wasm_bytes(memory).unwrap(),
                entries.wasm_bytes(memory).unwrap()
            );
            for artifact in [explicit, entries] {
                let metadata = artifact.metadata();
                assert_eq!(
                    (
                        metadata.backend_version,
                        metadata.abi_version,
                        metadata.profile
                    ),
                    (1, 1, 1)
                );
                assert_eq!((metadata.blocks, metadata.instructions), (1, 12));
                modules.push((0, artifact.wasm_bytes(memory).unwrap().to_vec()));
            }
            assert_eq!(page(memory, CODE), before);
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes, true);
                    describe(&mut engine, CODE, bytes.len(), entries);
                    let before = engine.arena().to_vec();
                    let code_before = page(engine.memory().unwrap(), CODE);
                    let id = compile(&mut engine, resident, entries).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_eq!(page(engine.memory().unwrap(), CODE), code_before);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(CODE), Access::Read)
                            .is_err()
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(DATA), Access::Read)
                            .is_err()
                    );
                    if resident {
                        engine.guard_resident(KEY, id).unwrap();
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        for offset in 0..=bytes.len() {
                            let found = engine.lookup_resident(CODE + offset as u32);
                            if starts.contains(&offset) {
                                assert_eq!(found.unwrap().get(), id);
                            } else {
                                assert!(found.is_err());
                            }
                        }
                        modules.push((2, engine.resident_bytes(id).unwrap().to_vec()));
                    } else {
                        assert_eq!(engine.generation(), id as u32);
                        engine.guard(KEY, id as u32).unwrap();
                        modules.push((1, engine.artifact_bytes().unwrap().to_vec()));
                    }
                }
            }
        }
    }
    assert_eq!(modules.len(), 96);
    validate_modules(&modules);
}

#[test]
fn newly_admitted_memory_neighbors_keep_original_ea_and_raw_count_identity() {
    let mut engine = code(CODE, &[0x90; 8], false);
    let mut identities = BTreeSet::new();
    for kind in KINDS {
        let field = u8::from(kind == RotateKind::Right) << 3;
        for (shape, (tail, address)) in [
            (
                &[0x00][..],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
            ),
            (
                &[0x40, 0x80][..],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: None,
                    scale: 1,
                    displacement: 0xffff_ff80,
                },
            ),
            (
                &[0x80, 0x78, 0x56, 0x34, 0x12][..],
                EffectiveAddress {
                    base: Some(Register32::Eax),
                    index: None,
                    scale: 1,
                    displacement: 0x1234_5678,
                },
            ),
            (
                &[0x05, 0x78, 0x56, 0x34, 0x12][..],
                EffectiveAddress {
                    base: None,
                    index: None,
                    scale: 1,
                    displacement: 0x1234_5678,
                },
            ),
            (
                &[0x04, 0x24][..],
                EffectiveAddress {
                    base: Some(Register32::Esp),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
            ),
            (
                &[0x04, 0x8a][..],
                EffectiveAddress {
                    base: Some(Register32::Edx),
                    index: Some(Register32::Ecx),
                    scale: 4,
                    displacement: 0,
                },
            ),
            (
                &[0x04, 0x8d, 0x78, 0x56, 0x34, 0x12][..],
                EffectiveAddress {
                    base: None,
                    index: Some(Register32::Ecx),
                    scale: 4,
                    displacement: 0x1234_5678,
                },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            for opcode in [0xc1, 0xd3] {
                for raw in if opcode == 0xc1 {
                    vec![0, 2, 32]
                } else {
                    vec![0]
                } {
                    let mut bytes = vec![opcode];
                    bytes.extend_from_slice(tail);
                    bytes[1] |= field;
                    if opcode == 0xc1 {
                        bytes.push(raw);
                    }
                    upload(&mut engine, CODE, &bytes);
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                    assert_eq!(
                        decoded.operation(),
                        &Operation::MemoryRotate {
                            kind,
                            address,
                            count: if opcode == 0xc1 {
                                ShiftCount::Immediate(raw)
                            } else {
                                ShiftCount::Cl
                            },
                        }
                    );
                    assert_eq!(
                        (decoded.length(), decoded.next_pc()),
                        (bytes.len() as u8, GuestAddress(CODE + bytes.len() as u32))
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                    assert!(identities.insert((
                        u8::from(kind == RotateKind::Right),
                        shape,
                        opcode,
                        raw
                    )));
                }
            }
        }
    }
    assert_eq!(identities.len(), 56);
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let mut cases = Vec::new();
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for kind in KINDS {
        for opcode in [0xc1, 0xd3] {
            let bytes = instruction(opcode, kind, 0, 2);
            for prefix in [
                0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
            ] {
                let expected = match prefix {
                    0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                        DecodeError::Unsupported(UnsupportedFeature::Segment)
                    }
                    0xf0 => DecodeError::InvalidEncoding,
                    _ => unsupported,
                };
                let mut prefixed = vec![prefix];
                prefixed.extend_from_slice(&bytes);
                cases.push((prefixed, expected));
            }
        }
        let field = u8::from(kind == RotateKind::Right) << 3;
        for operand in [0xc0 | field, 0x03 | field] {
            for raw in [0, 2, 32] {
                let mut bytes = vec![0xc0, operand, raw];
                bytes.insert(0, 0x66);
                cases.push((bytes, unsupported));
            }
            let mut cl = vec![0xd2, operand];
            cl.insert(0, 0x66);
            cases.push((cl, unsupported));
        }
        for opcode in [0xc1, 0xd3] {
            let mut bytes = vec![0xf0, opcode, 0x03 | field];
            if opcode == 0xc1 {
                bytes.push(2);
            }
            cases.push((bytes, DecodeError::InvalidEncoding));
        }
    }
    for field in [2, 3] {
        for operand in [0xc0 | field << 3, 0x03 | field << 3] {
            for raw in [0, 2, 32] {
                let mut bytes = vec![0xc1, operand, raw];
                if operand & 0xc0 == 0xc0 {
                    bytes.insert(0, 0x66);
                }
                cases.push((bytes, unsupported));
            }
            let mut bytes = vec![0xd3, operand];
            if operand & 0xc0 == 0xc0 {
                bytes.insert(0, 0x66);
            }
            cases.push((bytes, unsupported));
        }
    }
    for operand in [0xf0, 0x33] {
        cases.push((vec![0xc1, operand, 2], unsupported));
        cases.push((vec![0xd3, operand], unsupported));
    }
    assert_eq!(cases.len(), 84);
    assert_eq!(
        cases
            .iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        84
    );
    cases
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
    pages: [Vec<u8>; 2],
}

fn saved(engine: &EngineInstance, resident_id: u64) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(resident_id).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP].map(|pc| page(engine.memory().unwrap(), pc)),
    }
}

#[test]
fn excluded_profiles_and_late_failures_preserve_both_published_owners() {
    for (excluded, expected) in exclusions() {
        let mut bytes = instruction(0xc1, RotateKind::Left, 1, 2);
        bytes.extend_from_slice(&excluded);
        let failure = error(CODE + 3, expected);
        let engine = code(CODE, &bytes, false);
        let memory = engine.memory().unwrap();
        let before = page(memory, CODE);
        assert_eq!(
            decode_one(memory, GuestAddress(CODE + 3)).err(),
            Some(expected),
            "{excluded:02x?}"
        );
        assert_eq!(
            compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default()).err(),
            Some(failure)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(failure)
        );
        assert_eq!(page(memory, CODE), before);
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, false);
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                describe(&mut engine, KEEP, 3, false);
                engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = saved(&engine, keep);
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                assert_eq!(compile(&mut engine, resident, entries), Err(expected));
                assert_eq!(saved(&engine, keep), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA), Access::Read)
                        .is_err()
                );
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}

#[test]
fn exact_fetch_cuts_top_wrap_and_consumed_count_currency_are_preserved() {
    for kind in KINDS {
        for opcode in [0xc1, 0xd3] {
            let bytes = instruction(opcode, kind, 1, 255);
            let length = bytes.len() as u32;
            for pc in [0x2000 - length, u32::MAX - length + 1] {
                let engine = code(pc, &bytes, true);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &expected(kind, Register32::Ecx, opcode, 255)
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (length as u8, GuestAddress(pc.wrapping_add(length)))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                if pc < 0xffff_f000 {
                    assert!(
                        memory
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                } else {
                    for artifact in [
                        compile_region(memory, &[spec(pc, bytes.len())], CompileLimits::default())
                            .unwrap(),
                        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                            .unwrap(),
                    ] {
                        assert_eq!(artifact.metadata().instructions, 1);
                    }
                }
            }
            for present in 1..bytes.len() {
                for top in [false, true] {
                    let pc = if top {
                        u32::MAX - present as u32 + 1
                    } else {
                        0x2000 - present as u32
                    };
                    let engine = code(pc, &bytes[..present], true);
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
            let mut engine = code(pc, &bytes, false);
            engine.protect(0x2000, 1, 1).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(fetch_error(pc, 0x2000, length, FaultReason::Permission))
            );
            let mut engine = code(CODE, &bytes, false);
            engine.protect(CODE, 1, 1).unwrap();
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
            );
            for changed in 0..bytes.len() {
                for same in [false, true] {
                    for entries in [false, true] {
                        let pc = 0x1ffe;
                        let mut block = bytes.clone();
                        block.extend([0xeb, 0]);
                        let mut engine = code(pc, &block, false);
                        let decoded =
                            decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                        let memory = engine.memory().unwrap();
                        let artifacts = [
                            compile_region(
                                memory,
                                &[spec(pc, block.len())],
                                CompileLimits::default(),
                            )
                            .unwrap(),
                            compile_entry_region(
                                memory,
                                &[GuestAddress(pc)],
                                CompileLimits::default(),
                            )
                            .unwrap(),
                        ];
                        describe(&mut engine, pc, block.len(), entries);
                        let generation = compile(&mut engine, false, entries).unwrap() as u32;
                        describe(&mut engine, pc, block.len(), entries);
                        let resident = compile(&mut engine, true, entries).unwrap();
                        engine.map(DATA, 1, 3).unwrap();
                        upload(&mut engine, DATA, &[0xa5]);
                        for artifact in &artifacts {
                            artifact.wasm_bytes(engine.memory().unwrap()).unwrap();
                        }
                        engine.guard(KEY, generation).unwrap();
                        engine.guard_resident(KEY, resident).unwrap();
                        assert!(
                            engine
                                .memory()
                                .unwrap()
                                .is_code_current(decoded.code_snapshot())
                        );
                        let replacement = if same {
                            bytes[changed]
                        } else {
                            match changed {
                                0 => {
                                    if opcode == 0xc1 {
                                        0xd3
                                    } else {
                                        0xd1
                                    }
                                }
                                1 => bytes[1] ^ 8,
                                2 => 0,
                                _ => unreachable!(),
                            }
                        };
                        assert_eq!(replacement == bytes[changed], same);
                        upload(&mut engine, pc + changed as u32, &[replacement]);
                        assert!(
                            !engine
                                .memory()
                                .unwrap()
                                .is_code_current(decoded.code_snapshot())
                        );
                        for artifact in &artifacts {
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
                    }
                }
            }
        }
    }
}

#[test]
fn rotate_counts_keep_default_instruction_block_and_tight_wasm_limits() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for kind in KINDS {
        for opcode in [0xc1, 0xd3] {
            let instruction = instruction(opcode, kind, 1, 0);
            let mut bytes = instruction.repeat(63);
            bytes.extend([0xeb, 0]);
            let engine = code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            assert_eq!(
                prepare_region(memory, &[spec(CODE, bytes.len())], limits)
                    .unwrap()
                    .instruction_count(),
                64
            );
            assert_eq!(
                prepare_entry_region(memory, &[GuestAddress(CODE)], limits)
                    .unwrap()
                    .instruction_count(),
                64
            );
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes, true);
                    describe(&mut engine, CODE, bytes.len(), entries);
                    let before = engine.arena().to_vec();
                    compile(&mut engine, resident, entries).unwrap();
                    assert_eq!(engine.arena(), before);
                }
            }
            let mut bytes = instruction.repeat(64);
            bytes.extend([0x0f, 0x0b]);
            let engine = code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            assert_eq!(
                prepare_region(memory, &[spec(CODE, bytes.len())], limits).err(),
                Some(CompileError::InstructionLimit)
            );
            assert_eq!(
                prepare_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
                Some(CompileError::InstructionLimit)
            );
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes, true);
                    describe(&mut engine, CODE, bytes.len(), entries);
                    let before = engine.arena().to_vec();
                    let failure = if resident {
                        HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
                    } else {
                        HostError::Compile(CompileError::InstructionLimit)
                    };
                    assert_eq!(compile(&mut engine, resident, entries), Err(failure));
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.generation(), 0);
                }
            }
        }
    }
    let (bytes, _) = compact_bank(RotateKind::Left, 1);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let actual = compile_region(memory, &[spec(CODE, bytes.len())], limits)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap()
        .len();
    for tight in [actual, actual - 1, 1] {
        let tight_limits = CompileLimits {
            wasm_bytes: tight,
            ..limits
        };
        let explicit = compile_region(memory, &[spec(CODE, bytes.len())], tight_limits);
        let entries = compile_entry_region(memory, &[GuestAddress(CODE)], tight_limits);
        if tight == actual {
            assert!(explicit.is_ok());
            assert!(entries.is_ok());
        } else {
            assert_eq!(explicit.err(), Some(CompileError::WasmLimit));
            assert_eq!(entries.err(), Some(CompileError::WasmLimit));
        }
    }
    for invalid in [
        CompileLimits {
            blocks: 0,
            ..limits
        },
        CompileLimits {
            blocks: 9,
            ..limits
        },
        CompileLimits {
            instructions: 0,
            ..limits
        },
        CompileLimits {
            instructions: 65,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 0,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 65_537,
            ..limits
        },
    ] {
        assert_eq!(
            compile_region(memory, &[spec(CODE, bytes.len())], invalid).err(),
            Some(CompileError::InvalidLimits)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], invalid).err(),
            Some(CompileError::InvalidLimits)
        );
    }
    let atom = [0xc1, 0xc0, 2, 0xeb, 0];
    let bytes = atom.repeat(9);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let specs: Vec<_> = (0..9).map(|index| spec(CODE + index * 5, 5)).collect();
    let entries: Vec<_> = specs.iter().map(|spec| spec.entry).collect();
    for artifact in [
        compile_region(memory, &specs[..8], limits).unwrap(),
        compile_entry_region(memory, &entries[..8], limits).unwrap(),
    ] {
        assert_eq!(
            (artifact.metadata().blocks, artifact.metadata().instructions),
            (8, 16)
        );
    }
    assert_eq!(
        prepare_region(memory, &specs, limits).err(),
        Some(CompileError::InvalidBlocks)
    );
    assert_eq!(
        prepare_entry_region(memory, &entries, limits).err(),
        Some(CompileError::InvalidBlocks)
    );
}
