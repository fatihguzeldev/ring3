use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

const PC: u32 = 0x1000;
const KEY: u64 = 0x6555_5657_5859_5a5b;

#[test]
fn word_register_move_89_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let code = [0x66, 0x89, 0xc3, 0xeb, 0];
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(PC, code.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[PC.to_le_bytes(), (code.len() as u32).to_le_bytes()].concat());
    engine.compile(1).expect("word register 89 must compile");
}

#[test]
fn word_register_move_8b_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let code = [0x66, 0x8b, 0xc3, 0xeb, 0];
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(PC, code.len() as u32).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[PC.to_le_bytes(), (code.len() as u32).to_le_bytes()].concat());
    engine.compile(1).expect("word register 8b must compile");
}

use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
fn old_byte_dword_transfers_and_word_extensions_validate_and_capture_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_TRANSFER_MODULE_OUTPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_dir());
    }
    let cases: [(&str, &[u8]); 6] = [
        ("byte-89-direction", &[0x88, 0xc3]),
        ("byte-8b-direction", &[0x8a, 0xc3]),
        ("dword-89", &[0x89, 0xc3]),
        ("dword-8b", &[0x8b, 0xc3]),
        ("word-source-zero-extend", &[0x0f, 0xb7, 0xc3]),
        ("word-source-sign-extend", &[0x0f, 0xbf, 0xc3]),
    ];
    if let Some(directory) = &directory {
        for (name, _) in cases {
            assert!(!directory.join(format!("{name}.wasm")).exists());
        }
    }
    for (name, instruction) in cases {
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
    abi::arena::ARENA_SIZE,
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
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const KEEP: u32 = 0x3000;
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

fn describe(engine: &mut EngineInstance, specs: &[BlockSpec], entries: bool) {
    let stride = if entries { 4 } else { 8 };
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + specs.len() * stride];
    for (index, spec) in specs.iter().enumerate() {
        let at = index * stride;
        transfer[at..at + 4].copy_from_slice(&spec.entry.0.to_le_bytes());
        if !entries {
            transfer[at + 4..at + 8].copy_from_slice(&spec.byte_length.to_le_bytes());
        }
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

fn transfer(opcode: u8, destination: u8, source: u8) -> [u8; 3] {
    let modrm = if opcode == 0x89 {
        (source << 3) | destination
    } else {
        (destination << 3) | source
    };
    [0x66, opcode, 0xc0 | modrm]
}

fn bank(opcode: u8, half: u8) -> (Vec<u8>, Vec<BlockSpec>) {
    let mut bytes = vec![0xcc; 128];
    let mut specs = Vec::new();
    for slot in 0..4_u8 {
        for source in 0..8_u8 {
            let at = usize::from(slot) * 32 + usize::from(source) * 3;
            bytes[at..at + 3].copy_from_slice(&transfer(opcode, half * 4 + slot, source));
        }
        bytes[usize::from(slot) * 32 + 24..usize::from(slot) * 32 + 26].copy_from_slice(&[0xeb, 0]);
        specs.push(BlockSpec {
            entry: GuestAddress(PC + u32::from(slot) * 32),
            byte_length: 26,
        });
    }
    (bytes, specs)
}

#[test]
fn all_register_pairs_in_both_directions_have_exact_ir_and_six_pure_module_profiles() {
    let mut input = Vec::new();
    for opcode in [0x89, 0x8b] {
        for half in 0..2_u8 {
            let (bytes, specs) = bank(opcode, half);
            for profile in 0..6_u8 {
                let mut engine = fixture(PC, &bytes);
                for slot in 0..4_u8 {
                    for source in 0..8_u8 {
                        let pc = PC + u32::from(slot) * 32 + u32::from(source) * 3;
                        let instruction =
                            decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                        assert_eq!(
                            (instruction.length(), instruction.next_pc()),
                            (3, GuestAddress(pc + 3))
                        );
                        assert_eq!(
                            instruction.operation(),
                            &Operation::MoveWord {
                                destination: REGISTERS[usize::from(half * 4 + slot)],
                                source: WordValue::Register(REGISTERS[usize::from(source)])
                            }
                        );
                    }
                }
                let module = if profile < 2 {
                    let before = engine.arena().to_vec();
                    let compiled = if profile == 0 {
                        compile_region(engine.memory().unwrap(), &specs, CompileLimits::default())
                    } else {
                        compile_entry_region(
                            engine.memory().unwrap(),
                            &specs.iter().map(|spec| spec.entry).collect::<Vec<_>>(),
                            CompileLimits::default(),
                        )
                    }
                    .unwrap();
                    assert_eq!(
                        (compiled.metadata().blocks, compiled.metadata().instructions),
                        (4, 36)
                    );
                    let bytes = compiled
                        .wasm_bytes(engine.memory().unwrap())
                        .unwrap()
                        .to_vec();
                    assert_eq!(engine.arena(), before);
                    bytes
                } else {
                    let resident = profile >= 4;
                    let entries = profile % 2 == 1;
                    describe(&mut engine, &specs, entries);
                    let before = engine.arena().to_vec();
                    let id = bound(&mut engine, resident, entries, 4).unwrap();
                    assert_eq!(engine.arena(), before);
                    if resident {
                        engine.guard_resident(KEY, id).unwrap();
                        engine.resident_bytes(id).unwrap().to_vec()
                    } else {
                        engine.guard(KEY, id as u32).unwrap();
                        engine.artifact_bytes().unwrap().to_vec()
                    }
                };
                assert_eq!(engine.arena().len(), ARENA_SIZE);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5000), Access::Read)
                        .is_err()
                );
                input.push(profile);
                input.extend((module.len() as u32).to_le_bytes());
                input.extend(module);
            }
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert=require('node:assert/strict'),input=require('node:fs').readFileSync(0);let at=0,count=0;
while(at<input.length){const profile=input[at++],size=input.readUInt32LE(at);at+=4;const bytes=input.subarray(at,at+size);at+=size;
 assert.ok(WebAssembly.validate(bytes));const module=new WebAssembly.Module(bytes);
 assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},
 ...(profile<2?[]:[{module:'ring3',name:profile>=4?'guard_resident':'guard',kind:'function'}])]);
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
 let p=8;const leb=()=>{let n=0,s=0,b;do{b=bytes[p++];n|=(b&127)<<s;s+=7;}while(b&128);return n>>>0;};
 const sections=new Map();while(p<bytes.length){const id=bytes[p++],n=leb();sections.set(id,[p,p+n]);p+=n;}
 assert.deepEqual([...sections.keys()],[1,2,3,7,10]);p=sections.get(1)[0];const types=[];
 for(let i=leb();i>0;i--){assert.equal(bytes[p++],0x60);const n=leb(),params=[...bytes.subarray(p,p+n)];p+=n;const m=leb(),returns=[...bytes.subarray(p,p+m)];p+=m;types.push([params,returns]);}
 assert.deepEqual(types,[[Array(4).fill(127),[127]],...(profile<2?[]:[[Array(profile>=4?7:6).fill(127),[127]]])]);
 p=sections.get(10)[0];assert.equal(leb(),1);const end=leb()+p,locals=[];for(let n=leb();n>0;n--)locals.push([leb(),bytes[p++]]);
 assert.deepEqual(locals,[[16,127],[1,126]]);assert.equal(end,sections.get(10)[1]);count++;}
assert.equal(at,input.length);assert.equal(count,24);
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
fn strict_prefix_byte_memory_and_address_neighbors_keep_current_categories() {
    for opcode in [0x89, 0x8b] {
        for bytes in [
            vec![0x66, 0x66, opcode, 0xc3],
            vec![0x66, 0x67, opcode, 0xc3],
            vec![0x67, 0x66, opcode, 0xc3],
            vec![0x67, opcode, 0xc3],
            vec![0xf2, 0x66, opcode, 0xc3],
            vec![0xf3, 0x66, opcode, 0xc3],
            vec![0x66, 0x66, opcode, 0x03],
            vec![0x66, 0x67, opcode, 0x43, 0],
        ] {
            assert_eq!(
                decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            );
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
            assert_eq!(
                decode_one(
                    fixture(PC, &[prefix, 0x66, opcode, 0xc3]).memory().unwrap(),
                    GuestAddress(PC)
                )
                .err(),
                Some(DecodeError::Unsupported(UnsupportedFeature::Segment))
            );
        }
        assert_eq!(
            decode_one(
                fixture(PC, &[0xf0, 0x66, opcode, 0xc3]).memory().unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::InvalidEncoding)
        );
    }
    for opcode in [0x88, 0x8a] {
        assert_eq!(
            decode_one(
                fixture(PC, &[0x66, opcode, 0xc3]).memory().unwrap(),
                GuestAddress(PC)
            )
            .err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        );
    }
}

#[test]
fn three_consumed_bytes_cross_pages_and_top_fetch_faults_track_exact_currency() {
    for opcode in [0x89, 0x8b] {
        let bytes = [0x66, opcode, 0xc3];
        for pc in [PC, 0x1ffd, 0x1fff, 0xffff_fffd] {
            let engine = fixture(pc, &bytes);
            let instruction = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(
                (instruction.length(), instruction.next_pc()),
                (3, GuestAddress(pc.wrapping_add(3)))
            );
            for (offset, original) in bytes.iter().copied().enumerate() {
                for changing in [false, true] {
                    let mut engine = fixture(pc, &bytes);
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
        let mut denied = fixture(PC, &bytes);
        denied.protect(PC, 1, 1).unwrap();
        let second_missing = fixture(0x1fff, &[0x66]);
        let mut second_denied = fixture(0x1fff, &bytes);
        second_denied.protect(0x2000, 1, 1).unwrap();
        let third_missing = fixture(0x1ffe, &[0x66, opcode]);
        let mut third_denied = fixture(0x1ffe, &bytes);
        third_denied.protect(0x2000, 1, 1).unwrap();
        let top_second = fixture(u32::MAX, &[0x66]);
        let top_third = fixture(u32::MAX - 1, &[0x66, opcode]);
        for (engine, pc, address, reason, length) in [
            (&missing, PC, PC, FaultReason::Unmapped, 1),
            (&denied, PC, PC, FaultReason::Permission, 1),
            (&second_missing, 0x1fff, 0x2000, FaultReason::Unmapped, 2),
            (&second_denied, 0x1fff, 0x2000, FaultReason::Permission, 2),
            (&third_missing, 0x1ffe, 0x2000, FaultReason::Unmapped, 3),
            (&third_denied, 0x1ffe, 0x2000, FaultReason::Permission, 3),
            (
                &top_second,
                u32::MAX,
                u32::MAX,
                FaultReason::AddressOverflow,
                2,
            ),
            (
                &top_third,
                u32::MAX - 1,
                u32::MAX - 1,
                FaultReason::AddressOverflow,
                3,
            ),
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
                    length
                })
            );
        }
    }
}

#[test]
fn late_bad_decode_fetch_caps_and_data_changes_preserve_both_bound_owners() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65536
        }
    );
    for opcode in [0x89, 0x8b] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = fixture(PC, &[0x66, opcode, 0xc3, 0xeb, 0]);
                engine.protect(PC, 1, 7).unwrap();
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                let keep_specs = [BlockSpec {
                    entry: GuestAddress(KEEP),
                    byte_length: 3,
                }];
                describe(&mut engine, &keep_specs, false);
                let generation = engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                let prior = (
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(keep).unwrap().to_vec(),
                );
                let failures = [
                    (
                        PC,
                        vec![0x66, opcode, 0xc3, 0x66, 0x66, opcode, 0xc3, 0xeb, 0],
                        9,
                        CompileError::Instruction {
                            pc: GuestAddress(PC + 3),
                            cause: InstructionError::Decode(DecodeError::Unsupported(
                                UnsupportedFeature::Opcode,
                            )),
                        },
                    ),
                    (
                        0x1ffc,
                        vec![0x66, opcode, 0xc3, 0x66],
                        6,
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
                            let mut bytes = vec![0x66, opcode, 0xc3];
                            bytes.extend([0x90; 63]);
                            bytes.extend([0xeb, 0]);
                            bytes
                        },
                        68,
                        CompileError::InstructionLimit,
                    ),
                ];
                for (pc, bytes, length, failure) in failures {
                    upload(&mut engine, pc, &bytes);
                    describe(
                        &mut engine,
                        &[BlockSpec {
                            entry: GuestAddress(pc),
                            byte_length: length,
                        }],
                        entries,
                    );
                    let before = engine.arena().to_vec();
                    let expected = if resident {
                        HostError::Resident(RegistryError::Compile(failure))
                    } else {
                        HostError::Compile(failure)
                    };
                    assert_eq!(bound(&mut engine, resident, entries, 1), Err(expected));
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.generation(), generation);
                    assert_eq!(engine.artifact_bytes().unwrap(), prior.0);
                    assert_eq!(engine.resident_bytes(keep).unwrap(), prior.1);
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, keep).unwrap();
                }
                let mut exact = vec![0x66, opcode, 0xc3];
                exact.extend([0x90; 62]);
                exact.extend([0xeb, 0]);
                upload(&mut engine, PC, &exact);
                describe(
                    &mut engine,
                    &[BlockSpec {
                        entry: GuestAddress(PC),
                        byte_length: exact.len() as u32,
                    }],
                    entries,
                );
                let before = engine.arena().to_vec();
                let id = bound(&mut engine, resident, entries, 1).unwrap();
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
                upload(&mut engine, PC + 2, &[0xc3]);
                if resident {
                    assert!(engine.guard_resident(KEY, id).is_err());
                } else {
                    assert!(engine.guard(KEY, id as u32).is_err());
                }
            }
        }
    }
    let mut bytes = vec![0xcc; 128];
    for at in (0..128).step_by(16) {
        bytes[at..at + 5].copy_from_slice(&[0x66, 0x89, 0xc3, 0xeb, 0]);
    }
    let mut engine = fixture(PC, &bytes);
    let specs: Vec<_> = (0..8)
        .map(|i| BlockSpec {
            entry: GuestAddress(PC + i * 16),
            byte_length: 5,
        })
        .collect();
    describe(&mut engine, &specs, false);
    engine.compile(8).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile(9), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
}
