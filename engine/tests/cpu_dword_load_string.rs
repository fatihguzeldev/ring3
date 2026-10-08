use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

const PC: u32 = 0x1000;

#[test]
fn lodsd_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 3]
        .copy_from_slice(&[0xad, 0xeb, 0]);
    engine.upload(PC, 3).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&PC.to_le_bytes());
    descriptor[4..].copy_from_slice(&3_u32.to_le_bytes());
    engine.compile(1).expect("bare lodsd must compile");
}

use std::{
    fs::OpenOptions,
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn existing_lodsb_module_remains_byte_stable() {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 3]
        .copy_from_slice(&[0xac, 0xeb, 0]);
    engine.upload(PC, 3).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[PC.to_le_bytes(), 3_u32.to_le_bytes()].concat());
    engine.compile(1).unwrap();
    let bytes = engine.artifact_bytes().unwrap();
    let mut child = Command::new("node")
        .args([
            "-e",
            r#"
const assert = require('node:assert/strict'), bytes = require('node:fs').readFileSync(0);
assert.ok(WebAssembly.validate(bytes));
const module = new WebAssembly.Module(bytes);
assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env',name:'memory',kind:'memory'},
  ...['guard','read8'].map(name=>({module:'ring3',name,kind:'function'}))]);
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
    if let Some(path) = std::env::var_os("RING3_LODSB_MODULE_OUTPUT") {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const KEY: u64 = 0x5152_5354_5556_5758;
const KEEP: u32 = 0x3000;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}
fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(pc & !0xfff, 1, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(pc & !0xfff, 1, 4).unwrap();
    engine
}
fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(
        &[
            pc.to_le_bytes(),
            if entries { 0 } else { length as u32 }.to_le_bytes(),
        ]
        .concat(),
    );
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
fn typed_single_byte_fetch_wrap_and_consumed_code_currency_are_exact() {
    for pc in [PC, 0x1fff, u32::MAX] {
        let mut engine = fixture(pc, &[0xad]);
        let instruction = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(instruction.operation(), &Operation::LoadStringDword);
        assert_eq!(
            (instruction.length(), instruction.next_pc()),
            (1, GuestAddress(pc.wrapping_add(1)))
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(instruction.code_snapshot())
        );
        engine.protect(pc & !0xfff, 1, 7).unwrap();
        upload(&mut engine, pc, &[0xad]);
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(instruction.code_snapshot())
        );
    }
    let missing = EngineInstance::new(1, KEY).unwrap();
    assert_eq!(
        decode_one(missing.memory().unwrap(), GuestAddress(PC)).err(),
        Some(DecodeError::MemoryFault {
            pc: GuestAddress(PC),
            fault: MemoryFault {
                address: GuestAddress(PC),
                access: Access::Execute,
                reason: FaultReason::Unmapped
            },
            length: 1,
        })
    );
    let mut denied = fixture(PC, &[0xad]);
    denied.protect(PC, 1, 1).unwrap();
    assert_eq!(
        decode_one(denied.memory().unwrap(), GuestAddress(PC)).err(),
        Some(DecodeError::MemoryFault {
            pc: GuestAddress(PC),
            fault: MemoryFault {
                address: GuestAddress(PC),
                access: Access::Execute,
                reason: FaultReason::Permission
            },
            length: 1,
        })
    );
}

#[test]
fn strict_prefixes_and_both_standalone_profiles_stay_refused() {
    for (prefix, feature) in [
        (0xf2, UnsupportedFeature::RepeatedString),
        (0xf3, UnsupportedFeature::RepeatedString),
        (0x26, UnsupportedFeature::Segment),
        (0x2e, UnsupportedFeature::Segment),
        (0x36, UnsupportedFeature::Segment),
        (0x3e, UnsupportedFeature::Segment),
        (0x64, UnsupportedFeature::Segment),
        (0x65, UnsupportedFeature::Segment),
        (0x66, UnsupportedFeature::Opcode),
        (0x67, UnsupportedFeature::Opcode),
    ] {
        assert_eq!(
            decode_one(
                fixture(PC, &[prefix, if prefix == 0x66 { 0xac } else { 0xad }])
                    .memory()
                    .unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::Unsupported(feature))
        );
    }
    assert_eq!(
        decode_one(
            fixture(PC, &[0xf0, 0xad]).memory().unwrap(),
            GuestAddress(PC)
        )
        .err(),
        Some(DecodeError::InvalidEncoding)
    );
    let engine = fixture(PC, &[0x90, 0xad, 0xeb, 0]);
    for error in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 4,
            }],
            CompileLimits::default(),
        )
        .unwrap_err(),
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(PC)],
            CompileLimits::default(),
        )
        .unwrap_err(),
    ] {
        assert_eq!(
            error,
            CompileError::Instruction {
                pc: GuestAddress(PC + 1),
                cause: InstructionError::BackendUnsupported
            }
        );
    }
}

#[test]
fn four_bound_profiles_validate_existing_read32_imports_without_guest_execution() {
    let mut input = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(PC, &[0xad, 0xeb, 0]);
            describe(&mut engine, PC, 3, entries);
            let id = bound(&mut engine, resident, entries).unwrap();
            let bytes = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            input.push(u8::from(resident));
            input.extend((bytes.len() as u32).to_le_bytes());
            input.extend(bytes);
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'),input=require('node:fs').readFileSync(0);
let at=0,count=0;
while(at<input.length){const resident=input[at++],size=input.readUInt32LE(at);at+=4;
const bytes=input.subarray(at,at+size);at+=size;assert.ok(WebAssembly.validate(bytes));
const module=new WebAssembly.Module(bytes);
assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},
... [resident?'guard_resident':'guard','read32'].map(name=>({module:'ring3',name,kind:'function'}))]);count++;}
assert.equal(at,input.length);assert.equal(count,4);
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn late_refusals_caps_and_data_currency_preserve_bound_publications() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65536
        }
    );
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(PC, &[0xad, 0xeb, 0]);
            engine.protect(PC, 1, 7).unwrap();
            engine.map(KEEP, 1, 7).unwrap();
            upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
            describe(&mut engine, KEEP, 3, false);
            let generation = engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let prior = (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
            );
            let failures = [
                (
                    PC,
                    vec![0xad, 0x66, 0xac, 0xeb, 0],
                    5,
                    CompileError::Instruction {
                        pc: GuestAddress(PC + 1),
                        cause: InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Opcode,
                        )),
                    },
                ),
                (
                    0x1ffe,
                    vec![0xad, 0x0f],
                    3,
                    CompileError::Instruction {
                        pc: GuestAddress(0x1fff),
                        cause: InstructionError::Decode(DecodeError::MemoryFault {
                            pc: GuestAddress(0x1fff),
                            fault: MemoryFault {
                                address: GuestAddress(0x2000),
                                access: Access::Execute,
                                reason: FaultReason::Unmapped,
                            },
                            length: 2,
                        }),
                    },
                ),
                (
                    PC,
                    {
                        let mut bytes = vec![0x90; 64];
                        bytes[0] = 0xad;
                        bytes.extend([0xeb, 0]);
                        bytes
                    },
                    66,
                    CompileError::InstructionLimit,
                ),
            ];
            for (pc, bytes, length, failure) in failures {
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, length, entries);
                let before = engine.arena().to_vec();
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                assert_eq!(bound(&mut engine, resident, entries), Err(expected));
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.artifact_bytes().unwrap(), prior.0);
                assert_eq!(engine.resident_bytes(keep).unwrap(), prior.1);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            let mut limit = vec![0x90; 63];
            limit[0] = 0xad;
            limit.extend([0xeb, 0]);
            upload(&mut engine, PC, &limit);
            describe(&mut engine, PC, limit.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            engine.map(0x5000, 1, 3).unwrap();
            upload(&mut engine, 0x5000, &[0x81, 0xa5, 0x34, 0x12]);
            engine.protect(0x5000, 1, 2).unwrap();
            engine.unmap(0x5000, 1).unwrap();
            if resident {
                engine.guard_resident(KEY, id).unwrap();
            } else {
                engine.guard(KEY, id as u32).unwrap();
            }
            upload(&mut engine, PC, &[0xad]);
            if resident {
                assert!(engine.guard_resident(KEY, id).is_err());
            } else {
                assert!(engine.guard(KEY, id as u32).is_err());
            }
        }
    }
    let mut engine = fixture(PC, &[0xad, 0xeb, 0]);
    engine.protect(PC, 1, 7).unwrap();
    let mut bank = vec![0xcc; 128];
    for offset in (0..128).step_by(16) {
        bank[offset..offset + 3].copy_from_slice(&[0xad, 0xeb, 0]);
    }
    upload(&mut engine, PC, &bank);
    engine.protect(PC, 1, 4).unwrap();
    let descriptors: Vec<u8> = (0..8)
        .flat_map(|index| [(PC + index * 16).to_le_bytes(), 3_u32.to_le_bytes()].concat())
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine.compile(8).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile(9), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
}
