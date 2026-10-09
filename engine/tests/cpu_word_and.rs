use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x6758_595a_5b5c_5d5e;

fn compiled(instruction: &[u8]) -> EngineInstance {
    let mut code = instruction.to_vec();
    code.extend([0xeb, 0]);
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(PC, code.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[PC.to_le_bytes(), (code.len() as u32).to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("word and must compile through the public api");
    engine
}

#[test]
fn word_and_21_admits_in_bound_engine() {
    compiled(&[0x66, 0x21, 0xd8]);
}

#[test]
fn word_and_23_admits_in_bound_engine() {
    compiled(&[0x66, 0x23, 0xd8]);
}

#[test]
fn word_and_25_admits_in_bound_engine() {
    compiled(&[0x66, 0x25, 0xff, 0x7f]);
}

#[test]
fn word_and_81_admits_in_bound_engine() {
    compiled(&[0x66, 0x81, 0xe4, 0xff, 0x80]);
}

#[test]
fn word_and_83_admits_in_bound_engine() {
    compiled(&[0x66, 0x83, 0xe5, 0x80]);
}

#[test]
fn old_logical_and_word_paths_validate_and_optionally_capture_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_AND_MODULE_OUTPUT_DIR").map(PathBuf::from);
    let cases: [(&str, &[u8]); 4] = [
        ("byte-and", &[0x20, 0xe0]),
        ("dword-and", &[0x21, 0xc3]),
        ("word-test", &[0x66, 0x85, 0xc3]),
        ("word-add", &[0x66, 0x01, 0xc3]),
    ];
    if let Some(directory) = &directory {
        assert!(directory.is_dir() && !directory.is_symlink());
        for (name, _) in cases {
            assert!(!directory.join(format!("{name}.wasm")).exists());
        }
    }
    for (name, instruction) in cases {
        let engine = compiled(instruction);
        let bytes = engine.artifact_bytes().unwrap();
        let mut child = Command::new("node")
            .args([
                "-e",
                r#"
const assert = require('node:assert/strict'), bytes = require('node:fs').readFileSync(0);
assert.ok(WebAssembly.validate(bytes));
const module = new WebAssembly.Module(bytes);
assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env',name:'memory',kind:'memory'},
  {module:'ring3',name:'guard',kind:'function'}]);
assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run',kind:'function'}]);
"#,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        if let Some(directory) = &directory {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(format!("{name}.wasm")))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        }
    }
}

use ring3_engine::{
    abi::arena::TRANSFER_SIZE,
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
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
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
const FORMS: [&[u8]; 5] = [
    &[0x66, 0x21, 0xc3],
    &[0x66, 0x23, 0xc3],
    &[0x66, 0x25, 0xff, 0x7f],
    &[0x66, 0x81, 0xe4, 0x80, 0xff],
    &[0x66, 0x83, 0xe5, 0x80],
];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, bytes: usize, entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(bytes as u32).to_le_bytes());
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

#[test]
fn word_and_types_cover_both_directions_all_parents_and_signed_immediates() {
    let mut engine = fixture(PC, &[0x90]);
    for (left_index, left) in PARENTS.into_iter().enumerate() {
        for (right_index, right) in PARENTS.into_iter().enumerate() {
            for (opcode, modrm) in [
                (0x21, 0xc0 | (right_index as u8) << 3 | left_index as u8),
                (0x23, 0xc0 | (left_index as u8) << 3 | right_index as u8),
            ] {
                upload(&mut engine, PC, &[0x66, opcode, modrm]);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::LogicalWord {
                        kind: WordLogicalKind::And,
                        destination: left,
                        source: WordValue::Register(right)
                    }
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (3, GuestAddress(PC + 3))
                );
            }
        }
        for immediate in [0_u16, 0x7fff, 0x8000, 0xffff, 0x6766, 0xf3f0] {
            let mut bytes = vec![0x66, 0x81, 0xe0 | left_index as u8];
            bytes.extend(immediate.to_le_bytes());
            upload(&mut engine, PC, &bytes);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::LogicalWord {
                    kind: WordLogicalKind::And,
                    destination: left,
                    source: WordValue::Immediate(immediate)
                }
            );
            assert_eq!(decoded.length(), 5);
        }
        for immediate in [0_u8, 0x7f, 0x80, 0xff, 0x66, 0x67] {
            upload(
                &mut engine,
                PC,
                &[0x66, 0x83, 0xe0 | left_index as u8, immediate],
            );
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::LogicalWord {
                    kind: WordLogicalKind::And,
                    destination: left,
                    source: WordValue::Immediate(immediate as i8 as i16 as u16)
                }
            );
            assert_eq!(decoded.length(), 4);
        }
    }
    for immediate in [0_u16, 0x7fff, 0x8000, 0xffff, 0x6766, 0xf3f0] {
        let mut bytes = vec![0x66, 0x25];
        bytes.extend(immediate.to_le_bytes());
        upload(&mut engine, PC, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::LogicalWord {
                kind: WordLogicalKind::And,
                destination: Register32::Eax,
                source: WordValue::Immediate(immediate)
            }
        );
        assert_eq!(decoded.length(), 4);
    }
}

#[test]
fn word_and_strict_forms_fetch_and_consumed_code_currency_stay_exact() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for instruction in FORMS {
        for prefix in [0x66, 0x67, 0xf2, 0xf3, 0x64, 0xf0] {
            let bytes = [vec![prefix], instruction.to_vec()].concat();
            let expected = match prefix {
                0x64 => DecodeError::Unsupported(UnsupportedFeature::Segment),
                0xf0 => DecodeError::InvalidEncoding,
                _ => opcode,
            };
            assert_eq!(
                decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
                expected
            );
        }
        for cut in 1..instruction.len() {
            let pc = 0x2000 - cut as u32;
            assert_eq!(
                decode_one(
                    fixture(pc, &instruction[..cut]).memory().unwrap(),
                    GuestAddress(pc)
                )
                .unwrap_err(),
                DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped
                    },
                    length: cut as u32 + 1
                }
            );
        }
        let top = u32::MAX - (instruction.len() - 1) as u32;
        assert_eq!(
            decode_one(
                fixture(top, instruction).memory().unwrap(),
                GuestAddress(top)
            )
            .unwrap()
            .next_pc(),
            GuestAddress(0)
        );
        for (offset, original) in instruction.iter().copied().enumerate() {
            for changing in [false, true] {
                let mut engine = fixture(PC, instruction);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
                upload(
                    &mut engine,
                    PC + offset as u32,
                    &[original ^ u8::from(changing)],
                );
                assert!(
                    !engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
            }
        }
    }
    for bytes in [
        &[0x66, 0x21, 0x03][..],
        &[0x66, 0x23, 0x03],
        &[0x66, 0x81, 0x23, 0, 0],
        &[0x66, 0x83, 0x23, 0],
    ] {
        assert_eq!(
            decode_one(fixture(PC, bytes).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
            opcode
        );
    }
    for extension in [2, 3] {
        for opcode_byte in [0x81, 0x83] {
            let mut bytes = vec![0x66, opcode_byte, 0xc0 | extension << 3, 0];
            if opcode_byte == 0x81 {
                bytes.push(0);
            }
            assert_eq!(
                decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
                opcode
            );
        }
    }
}

fn validate_profile(bytes: &[u8], guard: &str) {
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'), b=require('node:fs').readFileSync(0), guard=process.argv[1];
const m=new WebAssembly.Module(b); assert.deepEqual(WebAssembly.Module.imports(m),
 [{module:'env',name:'memory',kind:'memory'},...(guard?[{module:'ring3',name:guard,kind:'function'}]:[])]);
assert.deepEqual(WebAssembly.Module.exports(m),[{name:'run',kind:'function'}]);
let at=8;const leb=()=>{let n=0,s=1;for(let i=0;i<5;i++){assert.ok(at<b.length);const v=b[at++];n+=(v&127)*s;if(!(v&128))return n;s*=128;}assert.fail('leb');};
while(at<b.length){const tag=b[at++],size=leb(),end=at+size;assert.ok(end<=b.length);
 if(tag===10){assert.equal(leb(),1);const body=leb();assert.equal(at+body,end);assert.equal(leb(),2);
  assert.deepEqual([leb(),b[at++],leb(),b[at++]],[16,127,1,126]);}at=end;}
assert.equal(at,b.length);
"#, guard]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn word_and_profiles_caps_and_late_failures_preserve_published_owners() {
    let bank = [FORMS.concat(), vec![0xeb, 0]].concat();
    let engine = fixture(PC, &bank);
    for compiled in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: bank.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(PC)],
            CompileLimits::default(),
        )
        .unwrap(),
    ] {
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 6)
        );
        validate_profile(compiled.wasm_bytes(engine.memory().unwrap()).unwrap(), "");
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(PC, &bank);
            describe(&mut engine, PC, bank.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            validate_profile(module, if resident { "guard_resident" } else { "guard" });
            engine.map(0x3000, 1, 7).unwrap();
            upload(&mut engine, 0x3000, &[0x90, 0xeb, 0]);
            describe(&mut engine, 0x3000, 3, false);
            engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let generation = engine.generation();
            let old = engine.artifact_bytes().unwrap().to_vec();
            let old_resident = engine.resident_bytes(keep).unwrap().to_vec();
            for (pc, bytes, length, fault_pc, expected) in [
                (
                    PC,
                    [FORMS[0].to_vec(), vec![0x66, 0x66, 0x21, 0xc0]].concat(),
                    7,
                    PC + 3,
                    InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                ),
                (
                    0x1ffc,
                    [FORMS[0].to_vec(), vec![0x66]].concat(),
                    5,
                    0x1fff,
                    InstructionError::Decode(DecodeError::MemoryFault {
                        pc: GuestAddress(0x1fff),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped,
                        },
                        length: 2,
                    }),
                ),
            ] {
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, length, entries);
                let arena = engine.arena().to_vec();
                let error = bound(&mut engine, resident, entries).unwrap_err();
                let observed = match error {
                    HostError::Compile(CompileError::Instruction { pc, cause })
                    | HostError::Resident(RegistryError::Compile(CompileError::Instruction {
                        pc,
                        cause,
                    })) => (pc, cause),
                    other => panic!("unexpected failure: {other:?}"),
                };
                assert_eq!(observed, (GuestAddress(fault_pc), expected));
                assert_eq!(engine.arena(), arena);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.artifact_bytes().unwrap(), old);
                assert_eq!(engine.resident_bytes(keep).unwrap(), old_resident);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            for nops in [62, 63] {
                if nops == 63 {
                    describe(&mut engine, 0x3000, 3, false);
                    engine.compile(1).unwrap();
                }
                let bytes = [FORMS[0].to_vec(), vec![0x90; nops], vec![0xeb, 0]].concat();
                upload(&mut engine, PC, &bytes);
                describe(&mut engine, PC, bytes.len(), entries);
                let arena = engine.arena().to_vec();
                let generation = engine.generation();
                let old = engine.artifact_bytes().unwrap().to_vec();
                let outcome = bound(&mut engine, resident, entries);
                if nops == 62 {
                    assert!(outcome.is_ok());
                } else {
                    assert_eq!(
                        outcome,
                        Err(if resident {
                            HostError::Resident(RegistryError::Compile(
                                CompileError::InstructionLimit,
                            ))
                        } else {
                            HostError::Compile(CompileError::InstructionLimit)
                        })
                    );
                    assert_eq!(engine.arena(), arena);
                    assert_eq!(engine.generation(), generation);
                    assert_eq!(engine.artifact_bytes().unwrap(), old);
                    assert_eq!(engine.resident_bytes(keep).unwrap(), old_resident);
                    engine.guard_resident(KEY, keep).unwrap();
                }
            }
        }
    }
}
