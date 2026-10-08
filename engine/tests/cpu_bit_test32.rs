use ring3_engine::process::EngineInstance;

#[test]
fn bit_test32_admits_before_jump() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x0f, 0xa3, 0xd0, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
    engine.upload(0x1000, 5).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&5_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register BT32 must admit before a jump");
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
            ir::{BitIndex, BitTestKind, Operation},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
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
const FORMS: [(BitTestKind, u8, u8); 4] = [
    (BitTestKind::Test, 0xa3, 4),
    (BitTestKind::Set, 0xab, 5),
    (BitTestKind::Reset, 0xb3, 6),
    (BitTestKind::Complement, 0xbb, 7),
];

fn register_instruction(opcode: u8, destination: u8, index: u8) -> Vec<u8> {
    vec![0x0f, opcode, 0xc0 | index << 3 | destination]
}

fn immediate_instruction(field: u8, destination: u8, raw: u8) -> Vec<u8> {
    vec![0x0f, 0xba, 0xc0 | field << 3 | destination, raw]
}

fn pair_block(destination: u8, index: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (_, opcode, field) in FORMS {
        bytes.extend(register_instruction(opcode, destination, index));
        bytes.extend(immediate_instruction(field, destination, 255));
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
    let value = 0, scale = 1;
    for (let i = 0; i < 5; i++) {
      assert.ok(at < bytes.length);
      const byte = bytes[at++]; value += (byte & 127) * scale;
      if (!(byte & 128)) { assert.ok(value <= 0xffffffff); return value; }
      scale *= 128;
    }
    assert.fail('invalid bounded unsigned LEB');
  };
  const vector = () => { const length = u(), start = at; at += length; assert.ok(at <= bytes.length); return [...bytes.subarray(start, at)]; };
  const text = () => Buffer.from(vector()).toString('utf8');
  const seen = [];
  while (at < bytes.length) {
    const id = bytes[at++], length = u(), end = at + length;
    assert.ok(end <= bytes.length && !seen.includes(id)); seen.push(id);
    if (id === 1) {
      const types = [];
      for (let n = u(); n > 0; n--) { assert.equal(bytes[at++], 0x60); types.push([vector(), vector()]); }
      const expected = [[Array(4).fill(0x7f), [0x7f]]];
      if (owner) expected.push([Array(owner === 1 ? 6 : 7).fill(0x7f), [0x7f]]);
      assert.deepEqual(types, expected);
    } else if (id === 2) {
      assert.equal(u(), owner ? 2 : 1); assert.equal(text(), 'env'); assert.equal(text(), 'memory');
      assert.equal(bytes[at++], 2); assert.equal(u(), 0); assert.equal(u(), 1);
      if (owner) { assert.equal(text(), 'ring3'); assert.equal(text(), owner === 1 ? 'guard' : 'guard_resident'); assert.equal(bytes[at++], 0); assert.equal(u(), 1); }
    } else if (id === 3) {
      assert.equal(u(), 1); assert.equal(u(), 0);
    } else if (id === 7) {
      assert.equal(u(), 1); assert.equal(text(), 'run'); assert.equal(bytes[at++], 0); assert.equal(u(), owner ? 1 : 0);
    } else if (id === 10) {
      assert.equal(u(), 1); const bodyLength = u(), bodyEnd = at + bodyLength, locals = [];
      for (let n = u(); n > 0; n--) locals.push([u(), bytes[at++]]);
      assert.deepEqual(locals, [[16,0x7f],[1,0x7e]]);
      assert.equal(bodyEnd, end); assert.equal(bytes[bodyEnd - 1], 0x0b); at = bodyEnd;
    } else assert.fail('unexpected pure module section');
    assert.equal(at, end);
  }
  assert.deepEqual(seen, [1,2,3,7,10]); count++;
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

fn assert_instruction(engine: &EngineInstance, bytes: &[u8], operation: Operation) {
    let memory = engine.memory().unwrap();
    let decoded = decode_one(memory, GuestAddress(CODE)).unwrap();
    assert_eq!(decoded.operation(), &operation);
    assert_eq!(
        (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
        (
            GuestAddress(CODE),
            bytes.len(),
            GuestAddress(CODE + bytes.len() as u32)
        )
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

#[test]
fn all_register_pairs_and_raw_immediates_preserve_exact_decode_identity() {
    let mut engine = code(CODE, &[0x90; 4], false);
    let mut register_forms = 0;
    let mut immediate_forms = 0;
    for (kind, opcode, field) in FORMS {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (index_number, index) in REGISTERS.into_iter().enumerate() {
                let bytes =
                    register_instruction(opcode, destination_index as u8, index_number as u8);
                upload(&mut engine, CODE, &bytes);
                assert_instruction(
                    &engine,
                    &bytes,
                    Operation::BitTest {
                        kind,
                        destination,
                        index: BitIndex::Register(index),
                    },
                );
                register_forms += 1;
            }
            for raw in 0..=u8::MAX {
                let bytes = immediate_instruction(field, destination_index as u8, raw);
                upload(&mut engine, CODE, &bytes);
                assert_instruction(
                    &engine,
                    &bytes,
                    Operation::BitTest {
                        kind,
                        destination,
                        index: BitIndex::Immediate(raw),
                    },
                );
                immediate_forms += 1;
            }
        }
    }
    assert_eq!((register_forms, immediate_forms), (256, 8192));
    assert_eq!(register_forms + immediate_forms, 8448);
}

#[test]
fn every_pair_admits_all_eight_forms_in_six_profiles_with_exact_pure_abi() {
    let mut modules = Vec::new();
    let starts = [0, 3, 7, 10, 14, 17, 21, 24, 28];
    for destination in 0..8 {
        for index in 0..8 {
            let bytes = pair_block(destination, index);
            assert_eq!(bytes.len(), 30);
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
                assert_eq!((metadata.blocks, metadata.instructions), (1, 9));
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
                    for address in [CODE, DATA] {
                        assert!(
                            engine
                                .memory()
                                .unwrap()
                                .resolve(GuestAddress(address), Access::Read)
                                .is_err()
                        );
                    }
                    if resident {
                        engine.guard_resident(KEY, id).unwrap();
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        for offset in 0..=bytes.len() as u32 {
                            if starts.contains(&offset) {
                                assert_eq!(
                                    engine.lookup_resident(CODE + offset).unwrap().get(),
                                    id
                                );
                            } else {
                                assert!(engine.lookup_resident(CODE + offset).is_err());
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
    assert_eq!(modules.len(), 384);
    validate_modules(&modules);
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = Vec::new();
    let memory_tails: [&[u8]; 7] = [
        &[0x00],
        &[0x43, 0xe0],
        &[0x83, 0x78, 0x56, 0x34, 0x12],
        &[0x05, 0x10, 0x50, 0, 0],
        &[0x04, 0x24],
        &[0x04, 0x8a],
        &[0x04, 0x8d, 0x10, 0x50, 0, 0],
    ];
    for (kind, register_opcode, field) in FORMS {
        for immediate in [false, true] {
            let bytes = if immediate {
                immediate_instruction(field, 3, 255)
            } else {
                register_instruction(register_opcode, 3, 2)
            };
            for prefix in [
                0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
            ] {
                let expected = match prefix {
                    0xf0 => DecodeError::InvalidEncoding,
                    0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                        DecodeError::Unsupported(UnsupportedFeature::Segment)
                    }
                    _ => opcode,
                };
                let mut prefixed = vec![prefix];
                prefixed.extend_from_slice(&bytes);
                cases.push((prefixed, expected));
            }
            for tail in memory_tails {
                let mut memory = vec![0x0f, if immediate { 0xba } else { register_opcode }];
                memory.extend_from_slice(tail);
                memory[2] |= (if immediate { field } else { 2 }) << 3;
                if immediate {
                    memory.push(255);
                }
                if kind == BitTestKind::Test {
                    memory.insert(0, 0x66);
                }
                cases.push((memory, opcode));
            }
            let mut locked = vec![
                0xf0,
                0x0f,
                if immediate { 0xba } else { register_opcode },
                3 | ((if immediate { field } else { 2 }) << 3),
            ];
            if immediate {
                locked.push(255);
            }
            cases.push((
                locked,
                if kind == BitTestKind::Test {
                    DecodeError::InvalidEncoding
                } else {
                    opcode
                },
            ));
        }
    }
    for field in 0..4 {
        cases.push((
            vec![0x0f, 0xba, 0xc3 | field << 3, 255],
            DecodeError::InvalidEncoding,
        ));
    }
    assert_eq!(cases.len(), 156);
    assert_eq!(
        cases
            .iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        156
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
fn memory_width_prefix_and_late_refusals_preserve_both_existing_owners() {
    let mut categories = [0; 3];
    for (excluded, expected) in exclusions() {
        categories[match expected {
            DecodeError::InvalidEncoding => 0,
            DecodeError::Unsupported(UnsupportedFeature::Segment) => 1,
            DecodeError::Unsupported(UnsupportedFeature::Opcode) => 2,
            _ => unreachable!(),
        }] += 1;
        let mut bytes = register_instruction(0xa3, 0, 2);
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
                let failure = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                assert_eq!(compile(&mut engine, resident, entries), Err(failure));
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
    assert_eq!(categories, [14, 48, 94]);
    let pc = 0x1ffc;
    let bytes = [0x0f, 0xa3, 0xd0, 0x0f];
    let failure = error(
        0x1fff,
        fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
    );
    let engine = code(pc, &bytes, false);
    let memory = engine.memory().unwrap();
    assert_eq!(
        compile_region(memory, &[spec(pc, 5)], CompileLimits::default()).err(),
        Some(failure)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()).err(),
        Some(failure)
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(pc, &bytes, false);
            engine.map(KEEP, 1, 7).unwrap();
            upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
            describe(&mut engine, KEEP, 3, false);
            engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            describe(&mut engine, pc, 5, entries);
            let before = saved(&engine, keep);
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(failure))
            } else {
                HostError::Compile(failure)
            };
            assert_eq!(compile(&mut engine, resident, entries), Err(expected));
            assert_eq!(saved(&engine, keep), before);
            engine.guard(KEY, before.generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
        }
    }
}

#[test]
fn exact_fetch_cuts_top_wrap_and_consumed_bytes_preserve_currency_boundaries() {
    let mut complete = 0;
    let mut cuts = 0;
    for (_, opcode, field) in FORMS {
        for bytes in [
            register_instruction(opcode, 1, 1),
            immediate_instruction(field, 1, 255),
        ] {
            let length = bytes.len() as u32;
            for pc in [0x2000 - length, u32::MAX - length + 1] {
                let engine = code(pc, &bytes, true);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
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
                complete += 1;
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
                    cuts += 1;
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
        }
    }
    assert_eq!((complete, cuts), (16, 40));
    let pc = 0x1ffd;
    let mut bytes = immediate_instruction(4, 1, 255);
    bytes.extend(pair_block(1, 1));
    let mut currency_cases = 0;
    for changed in 0..4 {
        for same_value in [false, true] {
            for entries in [false, true] {
                let mut engine = code(pc, &bytes, false);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                let memory = engine.memory().unwrap();
                let artifacts = [
                    compile_region(memory, &[spec(pc, bytes.len())], CompileLimits::default())
                        .unwrap(),
                    compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                        .unwrap(),
                ];
                describe(&mut engine, pc, bytes.len(), entries);
                let generation = compile(&mut engine, false, entries).unwrap() as u32;
                describe(&mut engine, pc, bytes.len(), entries);
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
                let replacement = if same_value {
                    bytes[changed as usize]
                } else {
                    [0x90, 0xbb, 0xe9, 127][changed as usize]
                };
                assert_eq!(replacement == bytes[changed as usize], same_value);
                upload(&mut engine, pc + changed, &[replacement]);
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
                currency_cases += 1;
            }
        }
    }
    assert_eq!(currency_cases, 16);
}

#[test]
fn bit_tests_keep_default_instruction_and_wasm_limits() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for (_, opcode, field) in FORMS {
        for instruction in [
            register_instruction(opcode, 1, 1),
            immediate_instruction(field, 1, 0),
        ] {
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
                    let expected = if resident {
                        HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
                    } else {
                        HostError::Compile(CompileError::InstructionLimit)
                    };
                    assert_eq!(compile(&mut engine, resident, entries), Err(expected));
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.generation(), 0);
                }
            }
        }
    }
    let bytes = pair_block(0, 2);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let tiny = CompileLimits {
        wasm_bytes: 1,
        ..limits
    };
    assert_eq!(
        compile_region(memory, &[spec(CODE, bytes.len())], tiny).err(),
        Some(CompileError::WasmLimit)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], tiny).err(),
        Some(CompileError::WasmLimit)
    );
}
