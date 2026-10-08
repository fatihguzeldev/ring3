use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

const PC: u32 = 0x1000;

#[test]
fn lodsw_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x5354_5556_5758_595a).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0x66, 0xad, 0xeb, 0]);
    engine.upload(PC, 4).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&PC.to_le_bytes());
    descriptor[4..].copy_from_slice(&4_u32.to_le_bytes());
    engine.compile(1).expect("exact lodsw must compile");
}

use std::{
    fs::OpenOptions,
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn existing_lodsb_and_lodsd_modules_validate_without_guest_execution() {
    for (opcode, name, helper) in [(0xac, "lodsb", "read8"), (0xad, "lodsd", "read32")] {
        let mut engine = EngineInstance::new(1, 0x5354_5556_5758_595a).unwrap();
        engine.map(PC, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 3]
            .copy_from_slice(&[opcode, 0xeb, 0]);
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
  ...['guard',process.argv[1]].map(name=>({module:'ring3',name,kind:'function'}))]);
assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run',kind:'function'}]);
"#,
                helper,
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
        if let Some(directory) = std::env::var_os("RING3_LODS_MODULE_OUTPUT_DIR") {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::path::PathBuf::from(directory).join(format!("{name}.wasm")))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        }
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

const KEY: u64 = 0x5354_5556_5758_595a;
const KEEP: u32 = 0x3000;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    let first = pc & !0xfff;
    let last = pc.checked_add(bytes.len() as u32 - 1).unwrap() & !0xfff;
    engine.map(first, 1, 7).unwrap();
    if last != first {
        engine.map(last, 1, 7).unwrap();
    }
    upload(&mut engine, pc, bytes);
    engine.protect(first, 1, 4).unwrap();
    if last != first {
        engine.protect(last, 1, 4).unwrap();
    }
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
fn typed_two_byte_fetch_wrap_faults_and_consumed_code_currency_are_exact() {
    for pc in [PC, 0x1ffe, 0x1fff, 0xffff_fffe] {
        let engine = fixture(pc, &[0x66, 0xad]);
        let instruction = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(instruction.operation(), &Operation::LoadStringWord);
        assert_eq!(
            (instruction.length(), instruction.next_pc()),
            (2, GuestAddress(pc.wrapping_add(2)))
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(instruction.code_snapshot())
        );
        for (offset, original) in [0x66, 0xad].into_iter().enumerate() {
            for changing in [false, true] {
                let mut engine = fixture(pc, &[0x66, 0xad]);
                let instruction = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                let address = pc + offset as u32;
                engine.protect(address & !0xfff, 1, 7).unwrap();
                upload(&mut engine, address, &[original ^ u8::from(changing)]);
                assert!(
                    !engine
                        .memory()
                        .unwrap()
                        .is_code_current(instruction.code_snapshot())
                );
            }
        }
    }
    let missing = EngineInstance::new(1, KEY).unwrap();
    let mut denied = fixture(PC, &[0x66, 0xad]);
    denied.protect(PC, 1, 1).unwrap();
    let second_missing = fixture(0x1fff, &[0x66]);
    let mut second_denied = fixture(0x1fff, &[0x66, 0xad]);
    second_denied.protect(0x2000, 1, 1).unwrap();
    let top = fixture(u32::MAX, &[0x66]);
    for (engine, pc, address, reason, length) in [
        (&missing, PC, PC, FaultReason::Unmapped, 1),
        (&denied, PC, PC, FaultReason::Permission, 1),
        (&second_missing, 0x1fff, 0x2000, FaultReason::Unmapped, 2),
        (&second_denied, 0x1fff, 0x2000, FaultReason::Permission, 2),
        (&top, u32::MAX, u32::MAX, FaultReason::AddressOverflow, 2),
    ] {
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(address),
                    access: Access::Execute,
                    reason
                },
                length,
            })
        );
    }
}

#[test]
fn exact_word_prefix_neighbors_and_both_standalone_profiles_stay_refused() {
    for bytes in [
        &[0x66, 0x66, 0xad][..],
        &[0x66, 0x67, 0xad],
        &[0x67, 0x66, 0xad],
        &[0x66, 0xac],
        &[0x66, 0xaa],
        &[0x67, 0xa5],
        &[0x66, 0xa6],
        &[0x66, 0xae],
        &[0x66, 0x9c],
        &[0x66, 0x90],
        &[0x66, 0x01, 0xc0],
    ] {
        assert_eq!(
            decode_one(fixture(PC, bytes).memory().unwrap(), GuestAddress(PC)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
    for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
        assert_eq!(
            decode_one(
                fixture(PC, &[prefix, 0x66, 0xad]).memory().unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Segment))
        );
    }
    for prefix in [0xf2, 0xf3] {
        assert_eq!(
            decode_one(
                fixture(PC, &[prefix, 0x66, 0xad]).memory().unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::RepeatedString))
        );
    }
    assert_eq!(
        decode_one(
            fixture(PC, &[0xf0, 0x66, 0xad]).memory().unwrap(),
            GuestAddress(PC)
        )
        .err(),
        Some(DecodeError::InvalidEncoding)
    );
    for (opcode, operation) in [
        (0xac, Operation::LoadStringByte),
        (0xad, Operation::LoadStringDword),
    ] {
        assert_eq!(
            decode_one(fixture(PC, &[opcode]).memory().unwrap(), GuestAddress(PC))
                .unwrap()
                .operation(),
            &operation
        );
    }
    let engine = fixture(PC, &[0x90, 0x66, 0xad, 0xeb, 0]);
    for error in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 5,
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
fn four_bound_profiles_validate_only_existing_read16_imports_without_guest_execution() {
    let mut input = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(PC, &[0x66, 0xad, 0xeb, 0]);
            describe(&mut engine, PC, 4, entries);
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
...[resident?'guard_resident':'guard','read16'].map(name=>({module:'ring3',name,kind:'function'}))]);
assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);count++;}
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
            let mut engine = fixture(PC, &[0x66, 0xad, 0xeb, 0]);
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
                    vec![0x66, 0xad, 0x66, 0xac, 0xeb, 0],
                    6,
                    CompileError::Instruction {
                        pc: GuestAddress(PC + 2),
                        cause: InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Opcode,
                        )),
                    },
                ),
                (
                    0x1ffd,
                    vec![0x66, 0xad, 0x66],
                    4,
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
                        let mut bytes = vec![0x66, 0xad];
                        bytes.extend([0x90; 63]);
                        bytes.extend([0xeb, 0]);
                        bytes
                    },
                    67,
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
            let mut limit = vec![0x66, 0xad];
            limit.extend([0x90; 62]);
            limit.extend([0xeb, 0]);
            upload(&mut engine, PC, &limit);
            describe(&mut engine, PC, limit.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            engine.map(0x5000, 1, 3).unwrap();
            upload(&mut engine, 0x5000, &[0x81, 0xa5]);
            engine.protect(0x5000, 1, 2).unwrap();
            engine.unmap(0x5000, 1).unwrap();
            if resident {
                engine.guard_resident(KEY, id).unwrap();
            } else {
                engine.guard(KEY, id as u32).unwrap();
            }
            upload(&mut engine, PC + 1, &[0xad]);
            if resident {
                assert!(engine.guard_resident(KEY, id).is_err());
            } else {
                assert!(engine.guard(KEY, id as u32).is_err());
            }
        }
    }
    let mut engine = fixture(PC, &[0x66, 0xad, 0xeb, 0]);
    engine.protect(PC, 1, 7).unwrap();
    let mut bank = vec![0xcc; 128];
    for offset in (0..128).step_by(16) {
        bank[offset..offset + 4].copy_from_slice(&[0x66, 0xad, 0xeb, 0]);
    }
    upload(&mut engine, PC, &bank);
    engine.protect(PC, 1, 4).unwrap();
    let descriptors: Vec<u8> = (0..8)
        .flat_map(|index| [(PC + index * 16).to_le_bytes(), 4_u32.to_le_bytes()].concat())
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine.compile(8).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile(9), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
}
