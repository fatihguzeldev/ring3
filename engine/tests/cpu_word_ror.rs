use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn compile_bound(bytes: &[u8]) -> Vec<u8> {
    let mut engine = EngineInstance::new(1, 0x574f_5244_524f_5221).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("register ROR must compile without data pages");
    engine.artifact_bytes().unwrap().to_vec()
}

#[test]
fn word_ror_one_admits_in_bound_engine() {
    compile_bound(&[0x66, 0xd1, 0xc8, 0xeb, 0]);
}

#[test]
fn word_ror_immediate_admits_in_bound_engine() {
    compile_bound(&[0x66, 0xc1, 0xc8, 0xff, 0xeb, 0]);
}

#[test]
fn word_ror_cl_admits_in_bound_engine() {
    compile_bound(&[0x66, 0xd3, 0xc8, 0xeb, 0]);
}

#[test]
fn existing_ror_modules_validate_and_optionally_capture() {
    let directory = std::env::var_os("RING3_WORD_ROR_MODULE_OUTPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    }
    let mut input = Vec::new();
    for (name, bytes) in [
        ("byte-one", &[0xd0, 0xc8, 0xeb, 0][..]),
        ("byte-immediate", &[0xc0, 0xc8, 0xff, 0xeb, 0][..]),
        ("byte-cl", &[0xd2, 0xc8, 0xeb, 0][..]),
        ("dword-one", &[0xd1, 0xc8, 0xeb, 0][..]),
        ("dword-immediate", &[0xc1, 0xc8, 0xff, 0xeb, 0][..]),
        ("dword-cl", &[0xd3, 0xc8, 0xeb, 0][..]),
    ] {
        let module = compile_bound(bytes);
        if let Some(directory) = &directory {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{name}.wasm")))
                .unwrap()
                .write_all(&module)
                .unwrap();
        }
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    let mut child = Command::new("node")
        .args(["-e", r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0;
for (let count = 0; count < 6; count++) {
  assert.ok(at + 4 <= input.length);
  const size = input.readUInt32LE(at); at += 4;
  assert.ok(size > 8 && at + size <= input.length);
  const module = new WebAssembly.Module(input.subarray(at, at + size)); at += size;
  assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env', name:'memory', kind:'memory'},
    {module:'ring3', name:'guard', kind:'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run', kind:'function'}]);
}
assert.equal(at, input.length);
"#])
        .stdin(Stdio::piped()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    assert!(child.wait().unwrap().success());
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Operation, RotateKind, ShiftCount},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::HostError,
};

const KEY: u64 = 0x574f_5244_524f_5221;
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

fn ror(opcode: u8, alias: u8, immediate: u8) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, 0xc8 | alias];
    if opcode == 0xc1 {
        bytes.push(immediate);
    }
    bytes
}

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut code = AddressSpace::new(pages + 1).unwrap();
    let range = PageRange::new(GuestAddress(base), pages).unwrap();
    code.map_zeroed(range, Permissions::ALL).unwrap();
    code.write(GuestAddress(pc), bytes).unwrap();
    code.protect(range, Permissions::EXECUTE).unwrap();
    code
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn prepare(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(pc, 1, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(pc, 1, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}

fn bound(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn artifact(engine: &EngineInstance, resident: bool, id: u64) -> Vec<u8> {
    if resident {
        engine.resident_bytes(id).unwrap().to_vec()
    } else {
        engine.artifact_bytes().unwrap().to_vec()
    }
}

#[test]
fn all_twenty_four_word_ror_shapes_keep_parent_and_original_count_source() {
    let mut shapes = 0;
    for opcode in [0xd1, 0xc1, 0xd3] {
        for (alias, destination) in PARENTS.into_iter().enumerate() {
            let bytes = ror(opcode, alias as u8, 0xff);
            let decoded = decode_one(&memory(0x1000, &bytes), GuestAddress(0x1000)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::RotateWord {
                    kind: RotateKind::Right,
                    destination,
                    count: match opcode {
                        0xd1 => ShiftCount::Immediate(1),
                        0xc1 => ShiftCount::Immediate(255),
                        0xd3 => ShiftCount::Cl,
                        _ => unreachable!(),
                    },
                }
            );
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
            shapes += 1;
        }
    }
    assert_eq!(shapes, 24);
    for immediate in 0..=255 {
        let bytes = ror(0xc1, 1, immediate);
        assert_eq!(
            decode_one(&memory(0x1000, &bytes), GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::RotateWord {
                kind: RotateKind::Right,
                destination: Register32::Ecx,
                count: ShiftCount::Immediate(immediate)
            }
        );
    }
}

#[test]
fn six_profiles_validate_all_ror_families_without_native_guest_execution() {
    let mut bytes = Vec::new();
    for opcode in [0xd1, 0xc1, 0xd3] {
        for alias in 0..8 {
            bytes.extend(ror(opcode, alias, 0xff));
        }
    }
    bytes.extend([0xeb, 0]);
    let code = memory(0x1000, &bytes);
    let mut input = Vec::new();
    for entries in [false, true] {
        let region = if entries {
            compile_entry_region(&code, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap()
        } else {
            compile_region(
                &code,
                &[BlockSpec {
                    entry: GuestAddress(0x1000),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
            .unwrap()
        };
        assert_eq!(
            (region.metadata().blocks, region.metadata().instructions),
            (1, 25)
        );
        let module = region.wasm_bytes(&code).unwrap();
        input.push(0);
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = prepare(0x1000, &bytes);
            describe(&mut engine, 0x1000, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
            let module = artifact(&engine, resident, id);
            input.push(if resident { 2 } else { 1 });
            input.extend((module.len() as u32).to_le_bytes());
            input.extend(module);
        }
    }
    let mut child = Command::new("node")
        .args([
            "-e",
            r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0;
for (let profile = 0; profile < 6; profile++) {
  const mode = input[at++];
  const length = input.readUInt32LE(at);
  at += 4;
  assert.ok(length > 8 && length <= 65536 && at + length <= input.length);
  const bytes = input.subarray(at, at + length);
  at += length;
  const module = new WebAssembly.Module(bytes);
  const imports = [{module:'env', name:'memory', kind:'memory'}];
  if (mode !== 0) {
    imports.push({module:'ring3', name:mode === 2 ? 'guard_resident' : 'guard', kind:'function'});
  }
  assert.deepEqual(WebAssembly.Module.imports(module), imports);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run', kind:'function'}]);
  let cursor = 8;
  function leb() {
    let value = 0;
    let scale = 1;
    for (let index = 0; index < 5; index++) {
      assert.ok(cursor < bytes.length);
      const byte = bytes[cursor++];
      value += (byte & 127) * scale;
      if (!(byte & 128)) return value;
      scale *= 128;
    }
    assert.fail('bounded LEB');
  }
  let found = false;
  while (cursor < bytes.length) {
    const id = bytes[cursor++];
    const size = leb();
    const end = cursor + size;
    assert.ok(end <= bytes.length);
    if (id === 10) {
      assert.equal(leb(), 1);
      const body = leb();
      assert.equal(cursor + body, end);
      assert.equal(leb(), 2);
      assert.equal(leb(), 16);
      assert.equal(bytes[cursor++], 127);
      assert.equal(leb(), 1);
      assert.equal(bytes[cursor++], 126);
      found = true;
    }
    cursor = end;
  }
  assert.ok(found);
}
assert.equal(at, input.length);
"#,
        ])
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

#[test]
fn exact_ror_prefix_fetch_top_and_consumed_source_boundaries() {
    for opcode in [0xd1, 0xc1, 0xd3] {
        let bytes = ror(opcode, 4, 0x66);
        for pc in [0x1000, 0x1fff, u32::MAX - (bytes.len() as u32 - 1)] {
            let mut code = memory(pc, &bytes);
            let decoded = decode_one(&code, GuestAddress(pc)).unwrap();
            assert_eq!(
                decoded.next_pc(),
                GuestAddress(pc.wrapping_add(bytes.len() as u32))
            );
            let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
            let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
            for (index, byte) in bytes.iter().enumerate() {
                code.protect(range, Permissions::ALL).unwrap();
                let old = decode_one(&code, GuestAddress(pc)).unwrap();
                code.write(GuestAddress(pc + index as u32), &[*byte])
                    .unwrap();
                assert!(!code.is_code_current(old.code_snapshot()));
                code.protect(range, Permissions::EXECUTE).unwrap();
            }
        }
        for cut in 1..bytes.len() {
            let pc = 0x2000 - cut as u32;
            let mut code = memory(pc, &bytes[..cut]);
            let expected = DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped,
                },
                length: cut as u32 + 1,
            };
            assert_eq!(decode_one(&code, GuestAddress(pc)).unwrap_err(), expected);
            code.map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
            assert!(matches!(
                decode_one(&code, GuestAddress(pc)),
                Err(DecodeError::MemoryFault {
                    fault: MemoryFault {
                        reason: FaultReason::Permission,
                        ..
                    },
                    ..
                })
            ));
            let top_pc = u32::MAX - cut as u32 + 1;
            assert!(matches!(
                decode_one(&memory(top_pc, &bytes[..cut]), GuestAddress(top_pc)),
                Err(DecodeError::MemoryFault {
                    fault: MemoryFault {
                        reason: FaultReason::AddressOverflow,
                        ..
                    },
                    ..
                })
            ));
        }
        let top_pc = u32::MAX - bytes.len() as u32 + 1;
        let top = memory(top_pc, &bytes);
        let limits = CompileLimits {
            instructions: 1,
            ..CompileLimits::default()
        };
        let explicit = compile_region(
            &top,
            &[BlockSpec {
                entry: GuestAddress(top_pc),
                byte_length: bytes.len() as u32,
            }],
            limits,
        )
        .unwrap();
        let entry = compile_entry_region(&top, &[GuestAddress(top_pc)], limits).unwrap();
        assert_eq!(explicit.metadata().instructions, 1);
        assert_eq!(
            explicit.wasm_bytes(&top).unwrap(),
            entry.wasm_bytes(&top).unwrap()
        );
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            let refused = [vec![prefix], bytes.clone()].concat();
            assert_eq!(
                decode_one(&memory(0x1000, &refused), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode)
            );
        }
        let locked = [vec![0xf0], bytes.clone()].concat();
        assert_eq!(
            decode_one(&memory(0x1000, &locked), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            let segmented = [vec![prefix], bytes.clone()].concat();
            assert_eq!(
                decode_one(&memory(0x1000, &segmented), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Segment)
            );
        }
        for modrm in [0x08, 0xd0, 0xd8, 0xf0] {
            let mut refused = vec![0x66, opcode, modrm];
            if opcode == 0xc1 {
                refused.push(0xff);
            }
            assert_eq!(
                decode_one(&memory(0x1000, &refused), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode)
            );
        }
    }
    for refused in [
        vec![0x66, 0xd0, 0xc8],
        vec![0x66, 0xc0, 0xc8, 0xff],
        vec![0x66, 0xd2, 0xc8],
        vec![0x66, 0x0f, 0xa4, 0xc8, 1],
        vec![0x66, 0x0f, 0xa5, 0xc8],
        vec![0x66, 0x0f, 0xac, 0xc8, 1],
        vec![0x66, 0x0f, 0xad, 0xc8],
    ] {
        assert_eq!(
            decode_one(&memory(0x1000, &refused), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        );
    }
    let bytes = [0x66, 0xc1, 0xc8, 0xff, 0xeb, 0];
    let mut code = memory(0x1000, &bytes);
    let region = compile_region(
        &code,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: bytes.len() as u32,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    code.map_zeroed(
        PageRange::new(GuestAddress(0x3000), 1).unwrap(),
        Permissions::ALL,
    )
    .unwrap();
    code.write(GuestAddress(0x3000), &[0x66]).unwrap();
    region.wasm_bytes(&code).unwrap();
    code.protect(
        PageRange::new(GuestAddress(0x1000), 1).unwrap(),
        Permissions::ALL,
    )
    .unwrap();
    code.write(GuestAddress(0x1003), &[0xff]).unwrap();
    assert_eq!(
        region.wasm_bytes(&code),
        Err(ArtifactError::CodeInvalidated)
    );
}

#[test]
fn caps_and_late_ror_refusals_preserve_full_arena_and_prior_owners() {
    for nops in [62, 63] {
        let bytes = [ror(0xd1, 0, 1), vec![0x90; nops], vec![0xeb, 0]].concat();
        let result = compile_region(
            &memory(0x1000, &bytes),
            &[BlockSpec {
                entry: GuestAddress(0x1000),
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
    let row = [0x66, 0xd1, 0xc8, 0xeb, 0];
    let code = memory(0x1000, &row.repeat(9));
    let specs: Vec<_> = (0..9)
        .map(|index| BlockSpec {
            entry: GuestAddress(0x1000 + index * row.len() as u32),
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
            let mut engine = prepare(0x1000, &row);
            for (pc, bytes) in [
                (0x3000, row.to_vec()),
                (0x5000, vec![0x66, 0xd1, 0xcc, 0x90, 0x0f, 0x0b, 0xeb, 0]),
            ] {
                engine.map(pc, 1, 7).unwrap();
                upload(&mut engine, pc, &bytes);
                engine.protect(pc, 1, 4).unwrap();
            }
            describe(&mut engine, 0x3000, row.len(), entries);
            let keep = bound(&mut engine, true, entries).unwrap();
            describe(&mut engine, 0x1000, row.len(), entries);
            let generation = bound(&mut engine, false, entries).unwrap();
            let old_replacement = artifact(&engine, false, generation);
            let old_resident = artifact(&engine, true, keep);
            let replacement_pointer = engine.artifact_bytes().unwrap().as_ptr() as usize;
            let resident_pointer = engine.resident_bytes(keep).unwrap().as_ptr() as usize;
            describe(&mut engine, 0x5000, 8, entries);
            let before = engine.arena().to_vec();
            let error = CompileError::Instruction {
                pc: GuestAddress(0x5004),
                cause: InstructionError::Decode(DecodeError::Unsupported(
                    UnsupportedFeature::Opcode,
                )),
            };
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(error))
            } else {
                HostError::Compile(error)
            };
            assert_eq!(bound(&mut engine, resident, entries).unwrap_err(), expected);
            assert_eq!(engine.arena(), before);
            assert_eq!(artifact(&engine, false, generation), old_replacement);
            assert_eq!(artifact(&engine, true, keep), old_resident);
            assert_eq!(
                engine.artifact_bytes().unwrap().as_ptr() as usize,
                replacement_pointer
            );
            assert_eq!(
                engine.resident_bytes(keep).unwrap().as_ptr() as usize,
                resident_pointer
            );
            assert_eq!(engine.generation() as u64, generation);
            engine.guard(KEY, generation as u32).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            assert_eq!(engine.lookup_resident(0x3000).unwrap().get(), keep);
            assert!(engine.lookup_resident(0x5000).is_err());
        }
    }
}
