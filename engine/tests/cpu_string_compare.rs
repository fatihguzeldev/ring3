use ring3_engine::process::EngineInstance;

const PC: u32 = 0x1000;

fn compile(opcode: u8) {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[opcode, 0xeb, 0]);
    engine.upload(PC, 3).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    engine.arena_mut().unwrap()[140..148]
        .copy_from_slice(&[PC.to_le_bytes(), 3_u32.to_le_bytes()].concat());
    engine
        .compile(1)
        .expect("bare string comparison must compile without data mappings");
}

#[test]
fn cmpsb_admits_in_bound_engine() {
    compile(0xa6);
}

#[test]
fn scasb_admits_in_bound_engine() {
    compile(0xae);
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{BlockSpec, CompileError, CompileLimits, InstructionError, compile_region},
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn memory(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn decode_fetch_and_prefix_barriers_remain_byte_exact() {
    for (opcode, operation) in [
        (0xa6, Operation::CompareStringByte),
        (0xae, Operation::ScanStringByte),
    ] {
        for pc in [PC, 0x1fff, u32::MAX] {
            let memory = memory(&[opcode], pc);
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.operation(), &operation);
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (1, GuestAddress(pc.wrapping_add(1)))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
        for (prefix, feature) in [
            (0xf3, UnsupportedFeature::RepeatedString),
            (0xf2, UnsupportedFeature::RepeatedString),
            (0x64, UnsupportedFeature::Segment),
            (0x26, UnsupportedFeature::Segment),
            (0x66, UnsupportedFeature::Opcode),
            (0x67, UnsupportedFeature::Opcode),
        ] {
            assert_eq!(
                decode_one(&memory(&[prefix, opcode], PC), GuestAddress(PC)).unwrap_err(),
                DecodeError::Unsupported(feature)
            );
        }
        assert_eq!(
            decode_one(&memory(&[0xf0, opcode], PC), GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let mut denied = memory(&[opcode], PC);
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
                ..
            })
        ));
    }
    for opcode in [0xa6, 0xae] {
        assert_eq!(
            decode_one(&memory(&[0x66, opcode], PC), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        );
    }
    assert!(matches!(
        decode_one(&AddressSpace::new(1).unwrap(), GuestAddress(PC)),
        Err(DecodeError::MemoryFault {
            pc: GuestAddress(PC),
            length: 1,
            ..
        })
    ));
}

fn bound_compile(
    engine: &mut EngineInstance,
    resident: bool,
    entries: bool,
    count: u32,
) -> Result<Option<u64>, ring3_engine::process::HostError> {
    if resident {
        let id = if entries {
            engine.compile_resident_entries(count, 0)?
        } else {
            engine.compile_resident(count)?
        };
        Ok(Some(id.get()))
    } else {
        if entries {
            engine.compile_entries(count, 0)?;
        } else {
            engine.compile(count)?;
        }
        Ok(None)
    }
}

fn module_bytes(engine: &EngineInstance, id: Option<u64>) -> Vec<u8> {
    match id {
        Some(id) => engine.resident_bytes(id).unwrap().to_vec(),
        None => engine.artifact_bytes().unwrap().to_vec(),
    }
}

#[test]
fn four_bound_profiles_validate_read_only_imports_and_pure_refusal() {
    let mut input = Vec::new();
    for opcode in [0xa6, 0xae] {
        let bytes = [opcode, 0xeb, 0];
        assert!(matches!(
            compile_region(
                &memory(&bytes, PC),
                &[BlockSpec {
                    entry: GuestAddress(PC),
                    byte_length: 3
                }],
                CompileLimits::default()
            ),
            Err(CompileError::Instruction {
                pc: GuestAddress(PC),
                cause: InstructionError::BackendUnsupported
            })
        ));
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
                engine.map(PC, 1, 7).unwrap();
                engine.arena_mut().unwrap()[140..143].copy_from_slice(&bytes);
                engine.upload(PC, 3).unwrap();
                engine.protect(PC, 1, 4).unwrap();
                engine.arena_mut().unwrap()[140..148]
                    .copy_from_slice(&[PC.to_le_bytes(), 3_u32.to_le_bytes()].concat());
                let id = bound_compile(&mut engine, resident, entries, 1).unwrap();
                let bytes = module_bytes(&engine, id);
                input.push(u8::from(resident));
                input.extend((bytes.len() as u32).to_le_bytes());
                input.extend(bytes);
            }
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert = require('node:assert/strict'), input = require('node:fs').readFileSync(0);
let at=0,count=0;
while(at<input.length) {
  const resident=input[at++],n=input.readUInt32LE(at); at+=4;
  const module=new WebAssembly.Module(input.subarray(at,at+n)); at+=n;
  assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env',name:'memory',kind:'memory'},
    ...[resident?'guard_resident':'guard','read8'].map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module), [{name:'run',kind:'function'}]); count++;
}
assert.equal(count,8);
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
fn late_decode_failure_preserves_full_arena_and_current_owner() {
    for resident in [false, true] {
        let mut engine = EngineInstance::new(2, 0x5152_5354_5556_5758).unwrap();
        for (address, bytes) in [
            (PC, &[0x90, 0xeb, 0][..]),
            (0x3000, &[0x90, 0xa6, 0x66, 0xae][..]),
        ] {
            engine.map(address, 1, 7).unwrap();
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
            engine.upload(address, bytes.len() as u32).unwrap();
            engine.protect(address, 1, 4).unwrap();
        }
        engine.arena_mut().unwrap()[140..148]
            .copy_from_slice(&[PC.to_le_bytes(), 3_u32.to_le_bytes()].concat());
        let id = bound_compile(&mut engine, resident, false, 1).unwrap();
        let old_module = module_bytes(&engine, id);
        engine.arena_mut().unwrap()[140..148]
            .copy_from_slice(&[0x3000_u32.to_le_bytes(), 4_u32.to_le_bytes()].concat());
        let before = engine.arena().to_vec();
        let error = bound_compile(&mut engine, resident, false, 1).unwrap_err();
        let error = format!("{error:?}");
        assert!(
            error.contains("GuestAddress(12290)") && error.contains("Unsupported(Opcode)"),
            "{error}"
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(module_bytes(&engine, id), old_module);
    }
}

#[test]
fn bound_default_instruction_and_block_caps_remain_exact() {
    for resident in [false, true] {
        for nops in [62, 63] {
            let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
            let bytes = [vec![0x90; nops], vec![0xa6, 0xeb, 0]].concat();
            engine.map(PC, 1, 7).unwrap();
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(&bytes);
            engine.upload(PC, bytes.len() as u32).unwrap();
            engine.protect(PC, 1, 4).unwrap();
            engine.arena_mut().unwrap()[140..148]
                .copy_from_slice(&[PC.to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat());
            assert_eq!(
                bound_compile(&mut engine, resident, false, 1).is_ok(),
                nops == 62
            );
        }
        let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
        engine.map(PC, 1, 7).unwrap();
        let bytes = [0xae, 0xeb, 0].repeat(8);
        engine.arena_mut().unwrap()[140..164].copy_from_slice(&bytes);
        engine.upload(PC, 24).unwrap();
        engine.protect(PC, 1, 4).unwrap();
        let descriptors: Vec<u8> = (0..8_u32)
            .flat_map(|index| [PC + index * 3, 3].into_iter().flat_map(u32::to_le_bytes))
            .collect();
        engine.arena_mut().unwrap()[140..204].copy_from_slice(&descriptors);
        let id = bound_compile(&mut engine, resident, false, 8).unwrap();
        let old_module = module_bytes(&engine, id);
        let before = engine.arena().to_vec();
        assert!(bound_compile(&mut engine, resident, false, 9).is_err());
        assert_eq!(engine.arena(), before);
        assert_eq!(module_bytes(&engine, id), old_module);
    }
}
