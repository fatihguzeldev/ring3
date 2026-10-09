use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn compile_bound(bytes: &[u8]) -> Vec<u8> {
    let mut engine = EngineInstance::new(1, 0x574d_494d_4d45_4449).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x1000), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("register WORD immediate MOV must compile without data pages");
    engine.artifact_bytes().unwrap().to_vec()
}

#[test]
fn word_opcode_immediate_admits_without_data_pages() {
    compile_bound(&[0x66, 0xb8, 0, 0x80, 0xeb, 0]);
}

#[test]
fn word_modrm_immediate_admits_without_data_pages() {
    compile_bound(&[0x66, 0xc7, 0xc4, 0xff, 0xff, 0xeb, 0]);
}

#[test]
fn existing_immediate_and_word_string_modules_validate_and_optionally_capture() {
    let directory = std::env::var_os("RING3_WORD_IMMEDIATE_BASELINE_DIR").map(PathBuf::from);
    let mut input = Vec::new();
    for (name, bytes) in [
        ("byte-opcode", &[0xb0, 0x80, 0xeb, 0][..]),
        ("byte-modrm", &[0xc6, 0xc4, 0xff, 0xeb, 0][..]),
        ("dword-opcode", &[0xbc, 0x78, 0x56, 0x34, 0x12, 0xeb, 0][..]),
        (
            "dword-modrm",
            &[0xc7, 0xc4, 0x78, 0x56, 0x34, 0x12, 0xeb, 0][..],
        ),
        ("word-load", &[0x66, 0xad, 0xeb, 0][..]),
        ("word-store", &[0x66, 0xab, 0xeb, 0][..]),
    ] {
        let module = compile_bound(bytes);
        if let Some(directory) = &directory {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{name}.wasm")))
                .unwrap();
            file.write_all(&module).unwrap();
        }
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    let mut child = Command::new("node")
        .args(["-e", r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0, count = 0;
for (const names of [[], [], [], [], ['read16'], ['store16']]) {
  const size = input.readUInt32LE(at); at += 4;
  assert.ok(size > 8 && at + size <= input.length);
  const module = new WebAssembly.Module(input.subarray(at, at + size)); at += size;
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...['guard', ...names].map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  count++;
}
assert.equal(count, 6); assert.equal(at, input.length);
"#])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
            ir::{Operation, WordValue},
        },
    },
    memory::{AddressSpace, FaultReason, MemoryFault, PageRange, Permissions},
    process::HostError,
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x574d_494d_4d45_4449;
const PARENTS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn immediate(modrm: bool, alias: u8, value: u16) -> Vec<u8> {
    let mut bytes = if modrm {
        vec![0x66, 0xc7, 0xc0 | alias]
    } else {
        vec![0x66, 0xb8 | alias]
    };
    bytes.extend(value.to_le_bytes());
    bytes
}

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 1).unwrap();
    let range = PageRange::new(GuestAddress(base), pages).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(range, Permissions::EXECUTE).unwrap();
    memory
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let request = engine.arena_mut().unwrap();
    request[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        request[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8].copy_from_slice(&length.to_le_bytes());
    }
}

fn bound(
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

fn artifact(engine: &EngineInstance, resident: bool, id: u64) -> Vec<u8> {
    if resident {
        engine.resident_bytes(id).unwrap().to_vec()
    } else {
        engine.artifact_bytes().unwrap().to_vec()
    }
}

fn validate_pure_modules(rows: &[(u8, Vec<u8>)]) {
    let mut input = Vec::new();
    for (mode, bytes) in rows {
        input.push(*mode);
        input.extend((bytes.len() as u32).to_le_bytes());
        input.extend(bytes);
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0, modules = 0;
function u(reader) {
  let n = 0, shift = 0;
  for (;;) {assert.ok(reader.at < reader.bytes.length && shift <= 28);
    const b = reader.bytes[reader.at++]; n += (b & 127) * 2 ** shift;
    if (!(b & 128)) return n; shift += 7;}
}
function one(r) {assert.ok(r.at < r.bytes.length); return r.bytes[r.at++];}
for (;;) {
  if (at === input.length) break;
  const mode = input[at++], length = input.readUInt32LE(at); at += 4;
  assert.ok([0, 1, 2].includes(mode) && length > 8 && length <= 65536 && at + length <= input.length);
  const bytes = input.subarray(at, at + length); at += length;
  const module = new WebAssembly.Module(bytes), guard = mode === 2 ? 'guard_resident' : 'guard';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...(mode ? [{module: 'ring3', name: guard, kind: 'function'}] : [])]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  const r = {bytes, at: 8}, sections = new Map();
  while (r.at < bytes.length) {const id = one(r), size = u(r); assert.ok(r.at + size <= bytes.length && !sections.has(id));
    sections.set(id, bytes.subarray(r.at, r.at + size)); r.at += size;}
  assert.deepEqual([...sections.keys()], [1, 2, 3, 7, 10]);
  const types = {bytes: sections.get(1), at: 0}; assert.equal(u(types), mode ? 2 : 1);
  for (const arity of mode ? [4, mode === 2 ? 7 : 6] : [4]) {
    assert.equal(one(types), 0x60); assert.equal(u(types), arity);
    for (let i = 0; i < arity; i++) assert.equal(one(types), 0x7f);
    assert.equal(u(types), 1); assert.equal(one(types), 0x7f);
  }
  assert.equal(types.at, types.bytes.length);
  const code = {bytes: sections.get(10), at: 0}; assert.equal(u(code), 1);
  const size = u(code); assert.equal(code.at + size, code.bytes.length);
  const body = {bytes: code.bytes.subarray(code.at), at: 0}; assert.equal(u(body), 2);
  assert.equal(u(body), 16); assert.equal(one(body), 0x7f);
  assert.equal(u(body), 1); assert.equal(one(body), 0x7e);
  assert.ok(body.at < body.bytes.length); modules++;
}
assert.equal(modules, 6); assert.equal(at, input.length);
"#]).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn typed_immediate_aliases_and_six_profiles_keep_native_publication_pure() {
    let mut observations = 0;
    for modrm in [false, true] {
        for alias in 0..8 {
            for value in [0, 0x7fff, 0x8000, 0xffff, 0x6667, 0xf0f3] {
                let bytes = immediate(modrm, alias, value);
                let memory = memory(PC, &bytes);
                let decoded = decode_one(&memory, GuestAddress(PC)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::MoveWord {
                        destination: PARENTS[alias as usize],
                        source: WordValue::Immediate(value),
                    }
                );
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (bytes.len(), GuestAddress(PC + bytes.len() as u32))
                );
                observations += 1;
            }
        }
    }
    assert_eq!(observations, 96);
    let mut bank = Vec::new();
    for alias in 0..8 {
        bank.extend(immediate(false, alias, 0x8000));
        bank.extend(immediate(true, alias, 0xffff));
    }
    bank.extend([0xeb, 0]);
    assert_eq!(bank.len(), 74);
    let code = memory(PC, &bank);
    let mut modules = Vec::new();
    for entries in [false, true] {
        let compiled = if entries {
            compile_entry_region(&code, &[GuestAddress(PC)], CompileLimits::default()).unwrap()
        } else {
            compile_region(
                &code,
                &[BlockSpec {
                    entry: GuestAddress(PC),
                    byte_length: 74,
                }],
                CompileLimits::default(),
            )
            .unwrap()
        };
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 17)
        );
        modules.push((0, compiled.wasm_bytes(&code).unwrap().to_vec()));
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, KEY).unwrap();
            engine.map(PC, 1, 7).unwrap();
            upload(&mut engine, PC, &bank);
            engine.protect(PC, 1, 4).unwrap();
            describe(&mut engine, PC, bank.len() as u32, entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries, 1).unwrap();
            assert_eq!(engine.arena(), before);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
            modules.push((
                if resident { 2 } else { 1 },
                artifact(&engine, resident, id),
            ));
        }
    }
    validate_pure_modules(&modules);
}

#[test]
fn exact_immediate_prefix_fetch_and_consumed_currency_boundaries() {
    for modrm in [false, true] {
        let bytes = immediate(modrm, 4, 0x6667);
        let length = bytes.len() as u32;
        for pc in [PC, 0x1fff, u32::MAX - length + 1] {
            let mut code = memory(pc, &bytes);
            let decoded = decode_one(&code, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(length)));
            let base = pc & !0xfff;
            let pages = (u64::from(pc - base) + u64::from(length)).div_ceil(4096) as u32;
            let range = PageRange::new(GuestAddress(base), pages).unwrap();
            for index in 0..bytes.len() {
                code.protect(range, Permissions::ALL).unwrap();
                let current = decode_one(&code, GuestAddress(pc)).unwrap();
                code.write(GuestAddress(pc + index as u32), &bytes[index..index + 1])
                    .unwrap();
                assert!(!code.is_code_current(current.code_snapshot()));
                code.protect(range, Permissions::EXECUTE).unwrap();
            }
        }
        for cut in 1..bytes.len() {
            let mut code = memory(0x2000 - cut as u32, &bytes[..cut]);
            assert!(matches!(
                decode_one(&code, GuestAddress(0x2000 - cut as u32)),
                Err(DecodeError::MemoryFault {
                    fault: MemoryFault {
                        reason: FaultReason::Unmapped,
                        ..
                    },
                    ..
                })
            ));
            code.map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
            assert!(matches!(
                decode_one(&code, GuestAddress(0x2000 - cut as u32)),
                Err(DecodeError::MemoryFault {
                    fault: MemoryFault {
                        reason: FaultReason::Permission,
                        ..
                    },
                    ..
                })
            ));
        }
        let top = memory(u32::MAX, &[0x66]);
        assert!(matches!(
            decode_one(&top, GuestAddress(u32::MAX)),
            Err(DecodeError::MemoryFault {
                fault: MemoryFault {
                    reason: FaultReason::AddressOverflow,
                    ..
                },
                ..
            })
        ));
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            let code = memory(PC, &[vec![prefix], bytes.clone()].concat());
            assert_eq!(
                decode_one(&code, GuestAddress(PC)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode)
            );
        }
        let locked = memory(PC, &[vec![0xf0], bytes.clone()].concat());
        assert_eq!(
            decode_one(&locked, GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let code = memory(PC, &[vec![prefix], bytes.clone()].concat());
            assert_eq!(
                decode_one(&code, GuestAddress(PC)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Segment)
            );
        }
        let code = memory(
            PC,
            &[
                bytes[..bytes.len() - 1].to_vec(),
                vec![bytes[bytes.len() - 1], 0xeb, 0],
            ]
            .concat(),
        );
        for cut in 1..length {
            assert!(matches!(
                compile_region(
                    &code,
                    &[BlockSpec {
                        entry: GuestAddress(PC),
                        byte_length: cut
                    }],
                    CompileLimits::default()
                ),
                Err(CompileError::Instruction {
                    cause: InstructionError::InvalidBlockEnd,
                    ..
                })
            ));
        }
    }
    for bytes in [
        vec![0x66, 0x66, 0xc7, 0, 0, 0],
        vec![0x66, 0xc7, 0xc8, 0, 0],
        vec![0x66, 0xb0, 1],
        vec![0x66, 0xc6, 0xc0, 1],
    ] {
        assert!(decode_one(&memory(PC, &bytes), GuestAddress(PC)).is_err());
    }
}

#[test]
fn instruction_block_caps_and_late_owner_failures_keep_complete_arena() {
    let instruction = immediate(false, 4, 0x8000);
    for nops in [62, 63] {
        let bytes = [instruction.clone(), vec![0x90; nops], vec![0xeb, 0]].concat();
        let code = memory(PC, &bytes);
        let result = compile_region(
            &code,
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        );
        if nops == 62 {
            assert_eq!(result.unwrap().metadata().instructions, 64);
        } else {
            assert!(matches!(result, Err(CompileError::InstructionLimit)));
        }
    }
    let row = [instruction, vec![0xeb, 0]].concat();
    let code = memory(PC, &row.repeat(9));
    let specs: Vec<_> = (0..9)
        .map(|index| BlockSpec {
            entry: GuestAddress(PC + index * row.len() as u32),
            byte_length: row.len() as u32,
        })
        .collect();
    assert_eq!(
        compile_region(&code, &specs[..8], CompileLimits::default())
            .unwrap()
            .metadata()
            .blocks,
        8
    );
    assert!(matches!(
        compile_region(&code, &specs, CompileLimits::default()),
        Err(CompileError::InvalidBlocks)
    ));
    assert!(matches!(
        compile_region(
            &code,
            &specs[..1],
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            }
        ),
        Err(CompileError::WasmLimit)
    ));
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(3, KEY).unwrap();
            for (pc, bytes) in [
                (PC, row.clone()),
                (0x3000, row.clone()),
                (
                    0x5000,
                    [row[..4].to_vec(), vec![0x90, 0x0f, 0x0b, 0xeb, 0]].concat(),
                ),
            ] {
                engine.map(pc, 1, 7).unwrap();
                upload(&mut engine, pc, &bytes);
                engine.protect(pc, 1, 4).unwrap();
            }
            describe(&mut engine, 0x3000, row.len() as u32, entries);
            let prior_resident = bound(&mut engine, true, entries, 1).unwrap();
            describe(&mut engine, PC, row.len() as u32, entries);
            let prior_replacement = bound(&mut engine, false, entries, 1).unwrap();
            let replacement = artifact(&engine, false, prior_replacement);
            let prior = artifact(&engine, true, prior_resident);
            describe(&mut engine, 0x5000, 9, entries);
            let before = engine.arena().to_vec();
            let error = CompileError::Instruction {
                pc: GuestAddress(0x5005),
                cause: InstructionError::Decode(DecodeError::Unsupported(
                    UnsupportedFeature::Opcode,
                )),
            };
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(error))
            } else {
                HostError::Compile(error)
            };
            assert_eq!(
                bound(&mut engine, resident, entries, 1).unwrap_err(),
                expected
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.generation() as u64, prior_replacement);
            assert_eq!(artifact(&engine, false, prior_replacement), replacement);
            assert_eq!(artifact(&engine, true, prior_resident), prior);
            engine.guard(KEY, prior_replacement as u32).unwrap();
            engine.guard_resident(KEY, prior_resident).unwrap();
            assert_eq!(
                bound(&mut engine, resident, entries, 9).unwrap_err(),
                HostError::InvalidRequest
            );
            assert_eq!(engine.arena(), before);
        }
    }
}
