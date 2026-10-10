use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn compile_bound(bytes: &[u8]) -> Vec<u8> {
    let mut engine = EngineInstance::new(1, 0x574f_5244_4f52_584f).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("register WORD OR/XOR must compile without data pages");
    engine.artifact_bytes().unwrap().to_vec()
}

#[test]
fn word_or_to_rm_register_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x09, 0xd8, 0xeb, 0]);
}

#[test]
fn word_or_to_reg_register_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x0b, 0xd8, 0xeb, 0]);
}

#[test]
fn word_or_accumulator_immediate_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x0d, 0, 0x80, 0xeb, 0]);
}

#[test]
fn word_or_modrm_immediate16_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x81, 0xcc, 0xff, 0xff, 0xeb, 0]);
}

#[test]
fn word_or_modrm_signed_immediate8_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x83, 0xcf, 0x80, 0xeb, 0]);
}

#[test]
fn word_xor_to_rm_register_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x31, 0xd8, 0xeb, 0]);
}

#[test]
fn word_xor_to_reg_register_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x33, 0xd8, 0xeb, 0]);
}

#[test]
fn word_xor_accumulator_immediate_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x35, 0xff, 0xff, 0xeb, 0]);
}

#[test]
fn word_xor_modrm_immediate16_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x81, 0xf4, 0xff, 0xff, 0xeb, 0]);
}

#[test]
fn word_xor_modrm_signed_immediate8_admits_in_bound_engine() {
    compile_bound(&[0x66, 0x83, 0xf7, 0xff, 0xeb, 0]);
}

#[test]
fn existing_or_xor_modules_validate_and_optionally_capture() {
    let directory = std::env::var_os("RING3_WORD_OR_XOR_BASELINE_DIR").map(PathBuf::from);
    let mut input = Vec::new();
    for (name, bytes) in [
        ("byte-or", &[0x08, 0xd8, 0xeb, 0][..]),
        ("dword-or", &[0x09, 0xd8, 0xeb, 0][..]),
        ("byte-xor", &[0x30, 0xd8, 0xeb, 0][..]),
        ("dword-xor", &[0x31, 0xd8, 0xeb, 0][..]),
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
for (let count = 0; count < 4; count++) {
  const size = input.readUInt32LE(at); at += 4;
  assert.ok(size > 8 && at + size <= input.length);
  const module = new WebAssembly.Module(input.subarray(at, at + size)); at += size;
  assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env', name:'memory', kind:'memory'},
    {module:'ring3', name:'guard', kind:'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run', kind:'function'}]);
}
assert.equal(at, input.length);
"#])
        .stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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
            ir::{Operation, WordLogicalKind, WordValue},
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::HostError,
};

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
const KEY: u64 = 0x574f_5244_4f52_584f;
const CHAIN: [u8; 40] = [
    0x66, 0x09, 0xd8, 0x66, 0x0b, 0xd8, 0x66, 0x0d, 0, 0x80, 0x66, 0x81, 0xcc, 0xff, 0xff, 0x66,
    0x83, 0xcf, 0x80, 0x66, 0x31, 0xd8, 0x66, 0x33, 0xd8, 0x66, 0x35, 0xff, 0xff, 0x66, 0x81, 0xf4,
    0xff, 0xff, 0x66, 0x83, 0xf7, 0xff, 0xeb, 0,
];
const KINDS: [(WordLogicalKind, u8, u8, u8, u8); 2] = [
    (WordLogicalKind::Or, 0x09, 0x0b, 0x0d, 1),
    (WordLogicalKind::Xor, 0x31, 0x33, 0x35, 6),
];

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

fn prepare_engine(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(pc, 1, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(pc, 1, 4).unwrap();
    engine
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        arena[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8].copy_from_slice(&length.to_le_bytes());
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

fn immediate16(accumulator: u8, extension: u8, alias: Option<u8>, value: u16) -> Vec<u8> {
    let mut bytes = alias.map_or_else(
        || vec![0x66, accumulator],
        |alias| vec![0x66, 0x81, 0xc0 | (extension << 3) | alias],
    );
    bytes.extend(value.to_le_bytes());
    bytes
}

#[test]
fn typed_word_or_xor_capture_all_parents_directions_and_signed_payloads() {
    let values = [0, 0x7fff, 0x8000, 0xffff, 0x6667, 0xf0f3];
    let mut count = 0;
    for (kind, to_rm, to_reg, accumulator, extension) in KINDS {
        for (destination, parent) in PARENTS.into_iter().enumerate() {
            for (source, source_parent) in PARENTS.into_iter().enumerate() {
                for opcode in [to_rm, to_reg] {
                    let modrm = if opcode == to_rm {
                        0xc0 | (source as u8) << 3 | destination as u8
                    } else {
                        0xc0 | (destination as u8) << 3 | source as u8
                    };
                    let decoded = decode_one(
                        &memory(0x1000, &[0x66, opcode, modrm]),
                        GuestAddress(0x1000),
                    )
                    .unwrap();
                    assert_eq!(
                        decoded.operation(),
                        &Operation::LogicalWord {
                            kind,
                            destination: parent,
                            source: WordValue::Register(source_parent),
                        }
                    );
                    assert_eq!(
                        (decoded.length(), decoded.next_pc()),
                        (3, GuestAddress(0x1003))
                    );
                    count += 1;
                }
            }
            for value in values {
                let decoded = decode_one(
                    &memory(
                        0x1000,
                        &immediate16(accumulator, extension, Some(destination as u8), value),
                    ),
                    GuestAddress(0x1000),
                )
                .unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::LogicalWord {
                        kind,
                        destination: parent,
                        source: WordValue::Immediate(value),
                    }
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (5, GuestAddress(0x1005))
                );
                count += 1;
            }
            for raw in [0_u8, 1, 0x7f, 0x80, 0xff, 0x66] {
                let decoded = decode_one(
                    &memory(
                        0x1000,
                        &[0x66, 0x83, 0xc0 | (extension << 3) | destination as u8, raw],
                    ),
                    GuestAddress(0x1000),
                )
                .unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::LogicalWord {
                        kind,
                        destination: parent,
                        source: WordValue::Immediate(raw as i8 as i16 as u16),
                    }
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (4, GuestAddress(0x1004))
                );
                count += 1;
            }
        }
        for value in values {
            let decoded = decode_one(
                &memory(0x1000, &immediate16(accumulator, extension, None, value)),
                GuestAddress(0x1000),
            )
            .unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::LogicalWord {
                    kind,
                    destination: Register32::Eax,
                    source: WordValue::Immediate(value),
                }
            );
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (4, GuestAddress(0x1004))
            );
            count += 1;
        }
    }
    assert_eq!(count, 460);
}

#[test]
fn six_profiles_validate_pure_word_or_xor_without_native_cpu_publication() {
    let code = memory(0x1000, &CHAIN);
    let mut input = Vec::new();
    for entries in [false, true] {
        let compiled = if entries {
            compile_entry_region(&code, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap()
        } else {
            compile_region(
                &code,
                &[BlockSpec {
                    entry: GuestAddress(0x1000),
                    byte_length: CHAIN.len() as u32,
                }],
                CompileLimits::default(),
            )
            .unwrap()
        };
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 11)
        );
        let bytes = compiled.wasm_bytes(&code).unwrap();
        input.push(0);
        input.extend((bytes.len() as u32).to_le_bytes());
        input.extend(bytes);
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = prepare_engine(0x1000, &CHAIN);
            describe(&mut engine, 0x1000, CHAIN.len() as u32, entries);
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
            let bytes = artifact(&engine, resident, id);
            input.push(if resident { 2 } else { 1 });
            input.extend((bytes.len() as u32).to_le_bytes());
            input.extend(bytes);
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'),b=require('node:fs').readFileSync(0);let at=0;
for(let i=0;i<6;i++) {const mode=b[at++],n=b.readUInt32LE(at);at+=4;
  assert.ok(n>8&&n<=65536&&at+n<=b.length);const bytes=b.subarray(at,at+n);at+=n;
  const m=new WebAssembly.Module(bytes);assert.deepEqual(WebAssembly.Module.imports(m),[{module:'env',name:'memory',kind:'memory'},
    ...(mode?[{module:'ring3',name:mode===2?'guard_resident':'guard',kind:'function'}]:[])]);
  assert.deepEqual(WebAssembly.Module.exports(m),[{name:'run',kind:'function'}]);
  let p=8;function leb(){let v=0,s=1;for(let j=0;j<5;j++){assert.ok(p<bytes.length);const x=bytes[p++];v+=(x&127)*s;if(!(x&128))return v;s*=128;}assert.fail('bounded LEB');}
  let found=false;while(p<bytes.length){const id=bytes[p++],size=leb(),end=p+size;assert.ok(end<=bytes.length);
    if(id===10){assert.equal(leb(),1);const body=leb();assert.equal(p+body,end);assert.equal(leb(),2);assert.equal(leb(),16);assert.equal(bytes[p++],127);assert.equal(leb(),1);assert.equal(bytes[p++],126);found=true;}
    p=end;}assert.ok(found);
}assert.equal(at,b.length);
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
fn exact_or_xor_prefix_fetch_and_consumed_source_currency() {
    let mut forms = Vec::new();
    for (_, to_rm, to_reg, accumulator, extension) in KINDS {
        forms.extend([
            vec![0x66, to_rm, 0xfc],
            vec![0x66, to_reg, 0xfc],
            immediate16(accumulator, extension, None, 0x6667),
            immediate16(accumulator, extension, Some(4), 0xf0f3),
            vec![0x66, 0x83, 0xc0 | (extension << 3) | 5, 0x80],
        ]);
    }
    for bytes in forms {
        let length = bytes.len() as u32;
        for pc in [0x1000, 0x1fff, u32::MAX - length + 1] {
            let mut code = memory(pc, &bytes);
            let decoded = decode_one(&code, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(length)));
            let range = PageRange::new(
                GuestAddress(pc & !0xfff),
                (u64::from(pc & 0xfff) + u64::from(length)).div_ceil(4096) as u32,
            )
            .unwrap();
            for index in 0..bytes.len() {
                code.protect(range, Permissions::ALL).unwrap();
                let old = decode_one(&code, GuestAddress(pc)).unwrap();
                code.write(GuestAddress(pc + index as u32), &bytes[index..index + 1])
                    .unwrap();
                assert!(!code.is_code_current(old.code_snapshot()));
                code.protect(range, Permissions::EXECUTE).unwrap();
            }
        }
        for cut in 1..length {
            let mut code = memory(0x2000 - cut, &bytes[..cut as usize]);
            assert_eq!(
                decode_one(&code, GuestAddress(0x2000 - cut)).unwrap_err(),
                DecodeError::MemoryFault {
                    pc: GuestAddress(0x2000 - cut),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped
                    },
                    length: cut + 1
                }
            );
            code.map_zeroed(
                PageRange::new(GuestAddress(0x2000), 1).unwrap(),
                Permissions::READ,
            )
            .unwrap();
            assert!(matches!(
                decode_one(&code, GuestAddress(0x2000 - cut)),
                Err(DecodeError::MemoryFault {
                    fault: MemoryFault {
                        reason: FaultReason::Permission,
                        ..
                    },
                    ..
                })
            ));
        }
        for prefix in [0x66, 0x67, 0xf2, 0xf3] {
            assert_eq!(
                decode_one(
                    &memory(0x1000, &[vec![prefix], bytes.clone()].concat()),
                    GuestAddress(0x1000)
                )
                .unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Opcode)
            );
        }
        assert_eq!(
            decode_one(
                &memory(0x1000, &[vec![0xf0], bytes.clone()].concat()),
                GuestAddress(0x1000)
            )
            .unwrap_err(),
            DecodeError::InvalidEncoding
        );
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            assert_eq!(
                decode_one(
                    &memory(0x1000, &[vec![prefix], bytes.clone()].concat()),
                    GuestAddress(0x1000)
                )
                .unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Segment)
            );
        }
    }
    let mut excluded = vec![
        vec![0x66, 0x66, 0x81, 0xd0, 0, 0],
        vec![0x66, 0x66, 0x83, 0xd8, 0],
    ];
    for (_, to_rm, to_reg, _, extension) in KINDS {
        excluded.extend([
            vec![0x66, 0x66, to_rm, 3],
            vec![0x66, 0x66, to_reg, 3],
            vec![0x66, 0x66, 0x81, (extension << 3) | 3, 0, 0],
            vec![0x66, 0x66, 0x83, (extension << 3) | 3, 0],
        ]);
    }
    for bytes in excluded {
        assert_eq!(
            decode_one(&memory(0x1000, &bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        );
    }
    assert!(matches!(
        decode_one(&memory(u32::MAX, &[0x66]), GuestAddress(u32::MAX)),
        Err(DecodeError::MemoryFault {
            fault: MemoryFault {
                reason: FaultReason::AddressOverflow,
                ..
            },
            ..
        })
    ));
}

#[test]
fn caps_and_late_refusals_preserve_complete_arena_and_prior_owners() {
    for nops in [62, 63] {
        let bytes = [vec![0x66, 0x09, 0xd8], vec![0x90; nops], vec![0xeb, 0]].concat();
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
    let row = [0x66, 0x09, 0xd8, 0xeb, 0];
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
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = prepare_engine(0x1000, &row);
            for (pc, bytes) in [
                (0x3000, row.to_vec()),
                (0x5000, vec![0x66, 0x09, 0xd8, 0x90, 0x0f, 0x0b, 0xeb, 0]),
            ] {
                engine.map(pc, 1, 7).unwrap();
                upload(&mut engine, pc, &bytes);
                engine.protect(pc, 1, 4).unwrap();
            }
            describe(&mut engine, 0x3000, row.len() as u32, entries);
            let resident_id = bound(&mut engine, true, entries, 1).unwrap();
            describe(&mut engine, 0x1000, row.len() as u32, entries);
            let generation = bound(&mut engine, false, entries, 1).unwrap();
            let old_replacement = artifact(&engine, false, generation);
            let old_resident = artifact(&engine, true, resident_id);
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
            assert_eq!(
                bound(&mut engine, resident, entries, 1).unwrap_err(),
                expected
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(artifact(&engine, false, generation), old_replacement);
            assert_eq!(artifact(&engine, true, resident_id), old_resident);
            assert_eq!(engine.generation() as u64, generation);
            engine.guard(KEY, generation as u32).unwrap();
            engine.guard_resident(KEY, resident_id).unwrap();
        }
    }
}
