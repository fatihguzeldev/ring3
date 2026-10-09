use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn compile_bound(bytes: &[u8]) -> Vec<u8> {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("exact bound string program must compile");
    engine.artifact_bytes().unwrap().to_vec()
}

#[test]
fn movsw_admits_in_bound_engine() {
    compile_bound(&[0x66, 0xa5, 0xeb, 0]);
}

#[test]
fn existing_movsb_movsd_modules_capture_without_guest_execution() {
    let directory = std::env::var_os("RING3_MOVS_BASELINE_DIR").map(PathBuf::from);
    let mut input = Vec::new();
    for (name, bytes) in [
        ("movsb", &[0xa4, 0xeb, 0][..]),
        ("movsd", &[0xa5, 0xeb, 0][..]),
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
    let mut child = Command::new("node").args(["-e", r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0, count = 0;
for (const names of [['guard', 'read8', 'store8'], ['guard', 'read32', 'store32']]) {
  const size = input.readUInt32LE(at); at += 4;
  assert.ok(size > 0 && at + size <= input.length);
  const module = new WebAssembly.Module(input.subarray(at, at + size)); at += size;
  assert.deepEqual(WebAssembly.Module.imports(module), [{module: 'env', name: 'memory', kind: 'memory'},
    ...names.map(name => ({module: 'ring3', name, kind: 'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name: 'run', kind: 'function'}]);
  count++;
}
assert.equal(count, 2); assert.equal(at, input.length);
"#]).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
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
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{
        Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
    },
    process::HostError,
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x5152_5354_5556_5758;

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let pages = (u64::from(pc & 0xfff) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages).unwrap();
    let range = PageRange::new(GuestAddress(pc & !0xfff), pages).unwrap();
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

fn describe(engine: &mut EngineInstance, pc: u32, length: u32) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[pc.to_le_bytes(), length.to_le_bytes()].concat());
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
fn exact_word_decode_fetch_snapshot_and_next_ip_wrap() {
    for pc in [PC, 0x1fff, 0xffff_fffe] {
        let memory = memory(pc, &[0x66, 0xa5]);
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::MoveStringWord);
        assert_eq!(decoded.pc(), GuestAddress(pc));
        assert_eq!(decoded.length(), 2);
        assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(2)));
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    let missing = AddressSpace::new(1).unwrap();
    let mut denied = memory(PC, &[0x66, 0xa5]);
    denied
        .protect(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let missing_second = memory(0x1fff, &[0x66]);
    let mut denied_second = memory(0x1fff, &[0x66, 0xa5]);
    denied_second
        .protect(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let top = memory(u32::MAX, &[0x66]);
    for (memory, pc, address, reason, length) in [
        (&missing, PC, PC, FaultReason::Unmapped, 1),
        (&denied, PC, PC, FaultReason::Permission, 1),
        (&missing_second, 0x1fff, 0x2000, FaultReason::Unmapped, 2),
        (&denied_second, 0x1fff, 0x2000, FaultReason::Permission, 2),
        (&top, u32::MAX, u32::MAX, FaultReason::AddressOverflow, 2),
    ] {
        assert_eq!(
            decode_one(memory, GuestAddress(pc)).unwrap_err(),
            DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(address),
                    access: Access::Execute,
                    reason
                },
                length,
            }
        );
    }
    let mut changed = memory(PC, &[0x66, 0xa5]);
    let decoded = decode_one(&changed, GuestAddress(PC)).unwrap();
    changed
        .protect(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    changed.write(GuestAddress(PC + 1), &[0xa4]).unwrap();
    assert!(!changed.is_code_current(decoded.code_snapshot()));
}

#[test]
fn exact_prefix_neighbors_and_both_standalone_profiles_stay_refused() {
    for bytes in [
        &[0x66, 0x66, 0xa5][..],
        &[0x66, 0x67, 0xa5],
        &[0x67, 0x66, 0xa5],
        &[0x67, 0xa5],
        &[0x66, 0xa4],
        &[0x66, 0xaa],
        &[0x66, 0x90],
    ] {
        assert_eq!(
            decode_one(&memory(PC, bytes), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        );
    }
    for (prefix, feature) in [
        (0xf2, UnsupportedFeature::RepeatedString),
        (0xf3, UnsupportedFeature::RepeatedString),
        (0x26, UnsupportedFeature::Segment),
        (0x2e, UnsupportedFeature::Segment),
        (0x36, UnsupportedFeature::Segment),
        (0x3e, UnsupportedFeature::Segment),
        (0x64, UnsupportedFeature::Segment),
        (0x65, UnsupportedFeature::Segment),
    ] {
        assert_eq!(
            decode_one(&memory(PC, &[prefix, 0x66, 0xa5]), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(feature)
        );
    }
    assert_eq!(
        decode_one(&memory(PC, &[0xf0, 0x66, 0xa5]), GuestAddress(PC)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
    for (opcode, operation) in [
        (0xa4, Operation::MoveStringByte),
        (0xa5, Operation::MoveStringDword),
    ] {
        assert_eq!(
            decode_one(&memory(PC, &[opcode]), GuestAddress(PC))
                .unwrap()
                .operation(),
            &operation
        );
    }
    let memory = memory(PC, &[0x90, 0x66, 0xa5, 0xeb, 0]);
    let expected = CompileError::Instruction {
        pc: GuestAddress(PC + 1),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            &memory,
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
        compile_entry_region(&memory, &[GuestAddress(PC)], CompileLimits::default()).unwrap_err(),
        expected
    );
}

#[test]
fn four_bound_profiles_validate_read16_store16_without_guest_execution() {
    let mut input = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, KEY).unwrap();
            engine.map(PC, 1, 7).unwrap();
            upload(&mut engine, PC, &[0x66, 0xa5, 0xeb, 0]);
            engine.protect(PC, 1, 4).unwrap();
            describe(&mut engine, PC, 4);
            engine.arena_mut().unwrap()[16..48].copy_from_slice(&[0x81; 32]);
            engine.arena_mut().unwrap()[4236..4364].fill(0xa5);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            input.push(u8::from(resident));
            input.extend((module.len() as u32).to_le_bytes());
            input.extend_from_slice(module);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            for access in [Access::Read, Access::Write] {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x5000), access)
                        .is_err()
                );
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
 ...[resident?'guard_resident':'guard','read16',resident?'store_resident16':'store16'].map(name=>({module:'ring3',name,kind:'function'}))]);
 assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
 let p=8;const leb=()=>{let n=0,s=0,b;do{b=bytes[p++];n|=(b&127)<<s;s+=7;}while(b&128);return n>>>0;};
 const sections=new Map();while(p<bytes.length){const id=bytes[p++],n=leb();sections.set(id,[p,p+n]);p+=n;}
 assert.deepEqual([...sections.keys()],[1,2,3,7,10]);p=sections.get(1)[0];const types=[];
 for(let i=leb();i>0;i--){assert.equal(bytes[p++],0x60);const n=leb(),params=[...bytes.subarray(p,p+n)];p+=n;const m=leb(),returns=[...bytes.subarray(p,p+m)];p+=m;types.push([params,returns]);}
 assert.deepEqual(types,[[Array(4).fill(127),[127]],[Array(resident?7:6).fill(127),[127]],[[127],[127]],[Array(resident?6:2).fill(127),[127]]]);
 p=sections.get(10)[0];assert.equal(leb(),1);const end=leb()+p,locals=[];for(let n=leb();n>0;n--)locals.push([leb(),bytes[p++]]);
 assert.deepEqual(locals,[[16,127],[1,126],[6,127]]);assert.equal(end,sections.get(10)[1]);count++;}
assert.equal(at,input.length);assert.equal(count,4);
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
fn caps_late_refusal_and_data_currency_preserve_bound_publications() {
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
            let mut engine = EngineInstance::new(3, KEY).unwrap();
            for (pc, bytes) in [
                (PC, &[0x66, 0xa5, 0xeb, 0][..]),
                (0x3000, &[0x90, 0x66, 0xaa, 0xeb, 0][..]),
                (0x4000, &[0x90, 0xeb, 0][..]),
            ] {
                engine.map(pc, 1, 7).unwrap();
                upload(&mut engine, pc, bytes);
                engine.protect(pc, 1, 4).unwrap();
            }
            describe(&mut engine, 0x4000, 3);
            let generation = engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let old = (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
            );
            describe(&mut engine, 0x3000, 5);
            let before = engine.arena().to_vec();
            let expected = CompileError::Instruction {
                pc: GuestAddress(0x3001),
                cause: InstructionError::Decode(DecodeError::Unsupported(
                    UnsupportedFeature::Opcode,
                )),
            };
            assert_eq!(
                bound(&mut engine, resident, entries).unwrap_err(),
                if resident {
                    HostError::Resident(RegistryError::Compile(expected))
                } else {
                    HostError::Compile(expected)
                }
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.artifact_bytes().unwrap(), old.0);
            assert_eq!(engine.resident_bytes(keep).unwrap(), old.1);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            assert!(engine.lookup_resident(0x3000).is_err());
        }
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        engine.map(PC, 1, 7).unwrap();
        engine.map(0x5000, 1, 3).unwrap();
        let mut exact = vec![0x66, 0xa5];
        exact.extend([0x90; 62]);
        exact.extend([0xeb, 0]);
        let mut too_many = vec![0x66, 0xa5];
        too_many.extend([0x90; 63]);
        too_many.extend([0xeb, 0]);
        upload(&mut engine, PC, &exact);
        upload(&mut engine, 0x1100, &too_many);
        describe(&mut engine, PC, exact.len() as u32);
        let id = bound(&mut engine, resident, false).unwrap();
        let saved = if resident {
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.artifact_bytes().unwrap().to_vec()
        };
        upload(&mut engine, 0x5000, &[0x81, 0x90]);
        if resident {
            engine.guard_resident(KEY, id).unwrap();
        } else {
            engine.guard(KEY, id as u32).unwrap();
        }
        describe(&mut engine, 0x1100, too_many.len() as u32);
        let before = engine.arena().to_vec();
        assert_eq!(
            bound(&mut engine, resident, false).unwrap_err(),
            if resident {
                HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
            } else {
                HostError::Compile(CompileError::InstructionLimit)
            }
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            },
            saved
        );
        upload(&mut engine, PC, &exact);
        assert!(
            if resident {
                engine.guard_resident(KEY, id)
            } else {
                engine.guard(KEY, id as u32)
            }
            .is_err()
        );
    }
    for resident in [false, true] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(PC, 1, 7).unwrap();
        let bytes: Vec<u8> = (0..9).flat_map(|_| [0x66, 0xa5, 0xeb, 0]).collect();
        upload(&mut engine, PC, &bytes);
        let descriptors: Vec<u8> = (0..9_u32)
            .flat_map(|index| [PC + index * 4, 4].into_iter().flat_map(u32::to_le_bytes))
            .collect();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
            .copy_from_slice(&descriptors);
        let id = if resident {
            engine.compile_resident(8).unwrap().get()
        } else {
            u64::from(engine.compile(8).unwrap())
        };
        let saved = if resident {
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.artifact_bytes().unwrap().to_vec()
        };
        let before = engine.arena().to_vec();
        let error = if resident {
            engine.compile_resident(9).unwrap_err()
        } else {
            engine.compile(9).unwrap_err()
        };
        assert_eq!(error, HostError::InvalidRequest);
        assert_eq!(engine.arena(), before);
        assert_eq!(
            if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            },
            saved
        );
    }
}
