use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

const PC: u32 = 0x1000;
const KEY: u64 = 0x5455_5657_5859_5a5b;

#[test]
fn cmpsw_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0x66, 0xa7, 0xeb, 0]);
    engine.upload(PC, 4).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[PC.to_le_bytes(), 4_u32.to_le_bytes()].concat());
    engine.compile(1).expect("exact cmpsw must compile");
}

#[test]
fn scasw_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0x66, 0xaf, 0xeb, 0]);
    engine.upload(PC, 4).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[PC.to_le_bytes(), 4_u32.to_le_bytes()].concat());
    engine.compile(1).expect("exact scasw must compile");
}

use std::{
    fs::OpenOptions,
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn existing_byte_and_dword_compare_scan_modules_validate_without_guest_execution() {
    for (opcode, name, helper) in [
        (0xa6, "cmpsb", "read8"),
        (0xae, "scasb", "read8"),
        (0xa7, "cmpsd", "read32"),
        (0xaf, "scasd", "read32"),
    ] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
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
        if let Some(directory) = std::env::var_os("RING3_COMPARE_MODULE_OUTPUT_DIR") {
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
    abi::arena::ARENA_SIZE,
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

const KEEP: u32 = 0x3000;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
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
fn typed_two_byte_fetch_faults_and_consumed_code_currency_are_exact() {
    for (opcode, operation) in [
        (0xa7, Operation::CompareStringWord),
        (0xaf, Operation::ScanStringWord),
    ] {
        for pc in [PC, 0x1ffe, 0x1fff, 0xffff_fffe] {
            let engine = fixture(pc, &[0x66, opcode]);
            let instruction = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(instruction.operation(), &operation);
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
            for (offset, original) in [0x66, opcode].into_iter().enumerate() {
                for changing in [false, true] {
                    let mut engine = fixture(pc, &[0x66, opcode]);
                    let instruction =
                        decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
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
        let mut denied = fixture(PC, &[0x66, opcode]);
        denied.protect(PC, 1, 1).unwrap();
        let second_missing = fixture(0x1fff, &[0x66]);
        let mut second_denied = fixture(0x1fff, &[0x66, opcode]);
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
}

#[test]
fn strict_prefix_neighbors_and_both_standalone_profiles_stay_refused() {
    for opcode in [0xa7, 0xaf] {
        for bytes in [
            vec![0x66, 0x66, opcode],
            vec![0x66, 0x67, opcode],
            vec![0x67, 0x66, opcode],
            vec![0x67, opcode],
        ] {
            assert_eq!(
                decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            );
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            assert_eq!(
                decode_one(
                    fixture(PC, &[prefix, 0x66, opcode]).memory().unwrap(),
                    GuestAddress(PC)
                )
                .err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Segment))
            );
        }
        for prefix in [0xf2, 0xf3] {
            assert_eq!(
                decode_one(
                    fixture(PC, &[prefix, 0x66, opcode]).memory().unwrap(),
                    GuestAddress(PC)
                )
                .err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::RepeatedString))
            );
        }
        assert_eq!(
            decode_one(
                fixture(PC, &[0xf0, 0x66, opcode]).memory().unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::InvalidEncoding)
        );
        let engine = fixture(PC, &[0x90, 0x66, opcode, 0xeb, 0]);
        let expected = CompileError::Instruction {
            pc: GuestAddress(PC + 1),
            cause: InstructionError::BackendUnsupported,
        };
        assert_eq!(
            compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(PC),
                    byte_length: 5
                }],
                CompileLimits::default()
            )
            .unwrap_err(),
            expected
        );
        assert_eq!(
            compile_entry_region(
                engine.memory().unwrap(),
                &[GuestAddress(PC)],
                CompileLimits::default()
            )
            .unwrap_err(),
            expected
        );
    }
    for opcode in [0xa6, 0xae] {
        assert_eq!(
            decode_one(
                fixture(PC, &[0x66, opcode]).memory().unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
}

#[test]
fn four_bound_profiles_validate_read16_locals_and_full_arena_without_guest_execution() {
    let mut input = Vec::new();
    for opcode in [0xa7, 0xaf] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = fixture(PC, &[0x66, opcode, 0xeb, 0]);
                describe(&mut engine, PC, 4, entries);
                let before = engine.arena().to_vec();
                let id = bound(&mut engine, resident, entries).unwrap();
                assert_eq!(engine.arena().len(), ARENA_SIZE);
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5000), Access::Read)
                        .is_err()
                );
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
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'),input=require('node:fs').readFileSync(0);
let at=0,count=0;
while(at<input.length){const resident=input[at++],size=input.readUInt32LE(at);at+=4;
 const bytes=input.subarray(at,at+size);at+=size;assert.ok(WebAssembly.validate(bytes));
 const module=new WebAssembly.Module(bytes);
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},
 ...[resident?'guard_resident':'guard','read16'].map(name=>({module:'ring3',name,kind:'function'}))]);
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
 let p=8;const leb=()=>{let n=0,s=0,b;do{b=bytes[p++];n|=(b&127)<<s;s+=7;}while(b&128);return n>>>0;};
 const sections=new Map();while(p<bytes.length){const id=bytes[p++],n=leb();sections.set(id,[p,p+n]);p+=n;}
 assert.deepEqual([...sections.keys()],[1,2,3,7,10]);p=sections.get(1)[0];const types=[];
 for(let i=leb();i>0;i--){assert.equal(bytes[p++],0x60);const n=leb(),params=[...bytes.subarray(p,p+n)];p+=n;const m=leb(),returns=[...bytes.subarray(p,p+m)];p+=m;types.push([params,returns]);}
 assert.deepEqual(types,[[Array(4).fill(127),[127]],[Array(resident?7:6).fill(127),[127]],[[127],[127]]]);
 p=sections.get(10)[0];assert.equal(leb(),1);const end=leb()+p,locals=[];for(let n=leb();n>0;n--)locals.push([leb(),bytes[p++]]);
 assert.deepEqual(locals,[[16,127],[1,126],[6,127]]);assert.equal(end,sections.get(10)[1]);count++;}
assert.equal(at,input.length);assert.equal(count,8);
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
fn late_refusals_caps_and_data_currency_preserve_both_bound_owners() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65536
        }
    );
    for opcode in [0xa7, 0xaf] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = fixture(PC, &[0x66, opcode, 0xeb, 0]);
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
                        vec![0x66, opcode, 0x66, 0xa6, 0xeb, 0],
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
                        vec![0x66, opcode, 0x66],
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
                            let mut bytes = vec![0x66, opcode];
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
                let mut exact = vec![0x66, opcode];
                exact.extend([0x90; 62]);
                exact.extend([0xeb, 0]);
                upload(&mut engine, PC, &exact);
                describe(&mut engine, PC, exact.len(), entries);
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
                upload(&mut engine, PC + 1, &[opcode]);
                if resident {
                    assert!(engine.guard_resident(KEY, id).is_err());
                } else {
                    assert!(engine.guard(KEY, id as u32).is_err());
                }
            }
        }
    }
    let mut engine = fixture(PC, &[0x66, 0xa7, 0xeb, 0]);
    engine.protect(PC, 1, 7).unwrap();
    let mut bank = vec![0xcc; 128];
    for offset in (0..128).step_by(16) {
        bank[offset..offset + 4].copy_from_slice(&[0x66, 0xa7, 0xeb, 0]);
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
