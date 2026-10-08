use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

const PC: u32 = 0x1000;

#[test]
fn stosd_admits_in_bound_engine() {
    for opcode in [0xaa, 0xab] {
        let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
        engine.map(PC, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 3]
            .copy_from_slice(&[opcode, 0xeb, 0]);
        engine.upload(PC, 3).unwrap();
        engine.protect(PC, 1, 4).unwrap();
        let descriptor = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
        descriptor[..4].copy_from_slice(&PC.to_le_bytes());
        descriptor[4..].copy_from_slice(&3_u32.to_le_bytes());
        engine
            .compile(1)
            .expect("bare accumulator store must compile");
        if let (0xaa, Some(path)) = (opcode, std::env::var_os("RING3_STOSB_MODULE_OUTPUT")) {
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            output.write_all(engine.artifact_bytes().unwrap()).unwrap();
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
    memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions},
    process::HostError,
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn descriptor(engine: &mut EngineInstance, pc: u32, length: u32) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[pc.to_le_bytes(), length.to_le_bytes()].concat());
}

#[test]
fn typed_single_byte_fetch_wrap_and_execute_permission_are_exact() {
    for pc in [PC, 0x1fff, u32::MAX] {
        let memory = memory(pc, &[0xab]);
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::StoreStringDword);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (1, GuestAddress(pc.wrapping_add(1)))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    let missing = AddressSpace::new(1).unwrap();
    assert!(matches!(
        decode_one(&missing, GuestAddress(PC)),
        Err(DecodeError::MemoryFault {
            pc: GuestAddress(PC),
            length: 1,
            ..
        })
    ));
    let mut denied = memory(PC, &[0xab]);
    denied
        .protect(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert!(matches!(
        decode_one(&denied, GuestAddress(PC)),
        Err(DecodeError::MemoryFault {
            pc: GuestAddress(PC),
            length: 1,
            fault: ring3_engine::memory::MemoryFault {
                access: Access::Execute,
                ..
            }
        })
    ));
    let mut changed = memory(PC, &[0xab]);
    let decoded = decode_one(&changed, GuestAddress(PC)).unwrap();
    changed
        .protect(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    changed.write(GuestAddress(PC), &[0x90]).unwrap();
    assert!(!changed.is_code_current(decoded.code_snapshot()));
}

#[test]
fn strict_prefixes_and_both_standalone_profiles_stay_refused() {
    for (prefix, feature) in [
        (0xf2, UnsupportedFeature::RepeatedString),
        (0xf3, UnsupportedFeature::RepeatedString),
        (0x64, UnsupportedFeature::Segment),
        (0x66, UnsupportedFeature::Opcode),
        (0x67, UnsupportedFeature::Opcode),
    ] {
        assert_eq!(
            decode_one(&memory(PC, &[prefix, 0xab]), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(feature)
        );
    }
    assert_eq!(
        decode_one(&memory(PC, &[0xf0, 0xab]), GuestAddress(PC)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
    let code = memory(PC, &[0x90, 0xab, 0xeb, 0]);
    for error in [
        compile_region(
            &code,
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 4,
            }],
            CompileLimits::default(),
        )
        .unwrap_err(),
        compile_entry_region(&code, &[GuestAddress(PC)], CompileLimits::default()).unwrap_err(),
    ] {
        assert_eq!(
            error,
            CompileError::Instruction {
                pc: GuestAddress(PC + 1),
                cause: InstructionError::BackendUnsupported
            }
        );
    }
    assert_eq!(
        decode_one(&memory(PC, &[0x66, 0xad]), GuestAddress(PC)).unwrap_err(),
        DecodeError::Unsupported(UnsupportedFeature::Opcode)
    );
}

#[test]
fn four_bound_profiles_validate_store_only_imports_without_guest_execution() {
    let mut input = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, 7).unwrap();
            engine.map(PC, 1, 7).unwrap();
            upload(&mut engine, PC, &[0xab, 0xeb, 0]);
            engine.protect(PC, 1, 4).unwrap();
            descriptor(&mut engine, PC, 3);
            let bytes = if resident {
                let id = if entries {
                    engine.compile_resident_entries(1, 0).unwrap()
                } else {
                    engine.compile_resident(1).unwrap()
                };
                engine.resident_bytes(id.get()).unwrap().to_vec()
            } else {
                if entries {
                    engine.compile_entries(1, 0).unwrap();
                } else {
                    engine.compile(1).unwrap();
                }
                engine.artifact_bytes().unwrap().to_vec()
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
const module=new WebAssembly.Module(input.subarray(at,at+size));at+=size;
assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},
...[resident?'guard_resident':'guard',resident?'store_resident32':'store32'].map(name=>({module:'ring3',name,kind:'function'}))]);
assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);count++;}
assert.equal(count,4);
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
fn instruction_block_caps_and_late_refusal_preserve_prior_bound_owners() {
    for resident in [false, true] {
        let mut engine = EngineInstance::new(2, 7).unwrap();
        for (pc, bytes) in [
            (PC, &[0xab, 0xeb, 0][..]),
            (0x3000, &[0x90, 0x66, 0xab, 0xeb, 0][..]),
        ] {
            engine.map(pc, 1, 7).unwrap();
            upload(&mut engine, pc, bytes);
            engine.protect(pc, 1, 4).unwrap();
        }
        descriptor(&mut engine, PC, 3);
        let id = if resident {
            engine.compile_resident(1).unwrap().get()
        } else {
            u64::from(engine.compile(1).unwrap())
        };
        let bytes = if resident {
            engine.resident_bytes(id).unwrap().to_vec()
        } else {
            engine.artifact_bytes().unwrap().to_vec()
        };
        descriptor(&mut engine, 0x3000, 5);
        let before = engine.arena().to_vec();
        let expected = CompileError::Instruction {
            pc: GuestAddress(0x3001),
            cause: InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        };
        let error = if resident {
            engine.compile_resident(1).unwrap_err()
        } else {
            engine.compile(1).unwrap_err()
        };
        assert_eq!(
            error,
            if resident {
                HostError::Resident(RegistryError::Compile(expected))
            } else {
                HostError::Compile(expected)
            }
        );
        assert_eq!(engine.arena(), before);
        if resident {
            assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
            engine.guard_resident(7, id).unwrap();
        } else {
            assert_eq!(engine.artifact_bytes().unwrap(), bytes);
            engine.guard(7, id as u32).unwrap();
        }
    }
    for count in [63_usize, 64] {
        let mut engine = EngineInstance::new(1, 7).unwrap();
        engine.map(PC, 1, 7).unwrap();
        let mut bytes = vec![0x90; count];
        bytes[0] = 0xab;
        bytes.extend([0xeb, 0]);
        upload(&mut engine, PC, &bytes);
        engine.protect(PC, 1, 4).unwrap();
        descriptor(&mut engine, PC, bytes.len() as u32);
        if count == 63 {
            engine.compile(1).unwrap();
        } else {
            assert_eq!(
                engine.compile(1),
                Err(HostError::Compile(CompileError::InstructionLimit))
            );
        }
    }
    let mut large = EngineInstance::new(1, 7).unwrap();
    large.map(PC, 1, 7).unwrap();
    let mut bytes = vec![0xa5; 63];
    bytes.extend([0xeb, 0]);
    upload(&mut large, PC, &bytes);
    large.protect(PC, 1, 4).unwrap();
    descriptor(&mut large, PC, bytes.len() as u32);
    assert_eq!(
        large.compile(1),
        Err(HostError::Compile(CompileError::WasmLimit))
    );
    let mut engine = EngineInstance::new(1, 7).unwrap();
    engine.map(PC, 1, 7).unwrap();
    let mut bytes = vec![0xcc; 128];
    for offset in (0..128).step_by(16) {
        bytes[offset..offset + 3].copy_from_slice(&[0xab, 0xeb, 0]);
    }
    upload(&mut engine, PC, &bytes);
    engine.protect(PC, 1, 4).unwrap();
    let rows: Vec<u8> = (0..8)
        .flat_map(|index| [(PC + index * 16).to_le_bytes(), 3_u32.to_le_bytes()].concat())
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + rows.len()]
        .copy_from_slice(&rows);
    engine.compile(8).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile(9), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
}
