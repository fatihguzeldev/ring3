use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

#[test]
fn bare_fnstcw_memory_admits_via_existing_public_api() {
    let bytes = [0xd9, 0x38, 0xeb, 0];
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0x1000_u32.to_le_bytes(), 4_u32.to_le_bytes()].concat());
    engine.compile(1).expect("bare FNSTCW memory must admit");
}

use ring3_engine::{
    cpu::{
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Operation},
        },
    },
    memory::{FaultReason, GuestAddress},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x1234_5678_9abc_def0;

fn code_at(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    engine.map(base, pages, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn request(engine: &mut EngineInstance, length: u32, entries: bool) {
    let words = if entries { vec![PC] } else { vec![PC, length] };
    for (index, value) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn control_memory_decodes_standard_32_bit_effective_addresses() {
    use Register32::{Eax, Ebp, Ecx, Edi, Esp};
    for (bytes, base, index, scale, displacement) in [
        (vec![0xd9, 0x38], Some(Eax), None, 1, 0),
        (vec![0xd9, 0x39], Some(Ecx), None, 1, 0),
        (vec![0xd9, 0x3c, 0x24], Some(Esp), None, 1, 0),
        (vec![0xd9, 0x7d, 0xf0], Some(Ebp), None, 1, 0xffff_fff0),
        (
            vec![0xd9, 0xbf, 0x78, 0x56, 0x34, 0x12],
            Some(Edi),
            None,
            1,
            0x1234_5678,
        ),
        (vec![0xd9, 0x3c, 0x88], Some(Eax), Some(Ecx), 4, 0),
        (
            vec![0xd9, 0x3c, 0xcd, 0x78, 0x56, 0x34, 0x12],
            None,
            Some(Ecx),
            8,
            0x1234_5678,
        ),
        (
            vec![0xd9, 0x3d, 0x78, 0x56, 0x34, 0x12],
            None,
            None,
            1,
            0x1234_5678,
        ),
    ] {
        let engine = code_at(PC, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::X87ControlToMemory {
                address: EffectiveAddress {
                    base,
                    index,
                    scale,
                    displacement
                }
            }
        );
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(PC + bytes.len() as u32));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    let engine = code_at(0x1fff, &[0xd9]);
    assert!(
        matches!(decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)), Err(DecodeError::MemoryFault { fault, length: 2, .. }) if fault.reason == FaultReason::Unmapped && fault.address == GuestAddress(0x2000))
    );
    let engine = code_at(0x1fff, &[0xd9, 0x38]);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x1fff))
            .unwrap()
            .next_pc(),
        GuestAddress(0x2001)
    );
}

#[test]
fn waited_prefixed_neighbors_and_standalone_memory_stay_refused() {
    for bytes in [
        vec![0x9b],
        vec![0x9b, 0xd9, 0x38],
        vec![0x9b, 0xdd, 0x38],
        vec![0xd9, 0xe8],
        vec![0xdd, 0xd8],
        vec![0xd8, 0xc1],
    ] {
        let engine = code_at(PC, &bytes);
        assert!(
            decode_one(engine.memory().unwrap(), GuestAddress(PC)).is_err(),
            "{bytes:02x?}"
        );
    }
    for prefix in [
        0x66, 0x67, 0xf0, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65,
    ] {
        let engine = code_at(PC, &[prefix, 0xd9, 0x38]);
        assert!(
            decode_one(engine.memory().unwrap(), GuestAddress(PC)).is_err(),
            "prefix{prefix:02x}"
        );
    }
    let engine = code_at(PC, &[0xd9, 0x38, 0xeb, 0]);
    for result in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 4,
            }],
            CompileLimits::default(),
        ),
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(PC)],
            CompileLimits::default(),
        ),
    ] {
        assert!(matches!(
            result,
            Err(CompileError::Instruction {
                pc: GuestAddress(PC),
                cause: InstructionError::BackendUnsupported
            })
        ));
    }
}

#[test]
fn four_bound_profiles_validate_word_store_and_shared_helper_types() {
    let mut input = Vec::new();
    for (mixed, bytes) in [
        (false, vec![0xd9, 0x38, 0xeb, 0]),
        (
            true,
            vec![
                0xd9, 0x38, 0x0f, 0xb7, 0x11, 0x8a, 0x03, 0x88, 0x03, 0xeb, 0,
            ],
        ),
    ] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code_at(PC, &bytes);
                request(&mut engine, bytes.len() as u32, entries);
                let before = engine.arena().to_vec();
                let module = if resident {
                    let id = if entries {
                        engine.compile_resident_entries(1, 0)
                    } else {
                        engine.compile_resident(1)
                    }
                    .unwrap()
                    .get();
                    engine.guard_resident(KEY, id).unwrap();
                    engine.resident_bytes(id).unwrap().to_vec()
                } else {
                    let generation = if entries {
                        engine.compile_entries(1, 0)
                    } else {
                        engine.compile(1)
                    }
                    .unwrap();
                    engine.guard(KEY, generation).unwrap();
                    engine.artifact_bytes().unwrap().to_vec()
                };
                assert_eq!(engine.arena(), before);
                input.extend([u8::from(mixed), u8::from(resident)]);
                input.extend((module.len() as u32).to_le_bytes());
                input.extend(module);
            }
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert = require('node:assert/strict'), input = require('node:fs').readFileSync(0);
let at=0, count=0;
while(at<input.length) {
  const mixed=input[at++], resident=input[at++], length=input.readUInt32LE(at); at+=4;
  const bytes=input.subarray(at,at+length); at+=length;
  assert.ok(WebAssembly.validate(bytes)); const module=new WebAssembly.Module(bytes);
  const names=[resident?'guard_resident':'guard',...(mixed?['read8','read16',resident?'store_resident8':'store8']:[]),resident?'store_resident16':'store16'];
  assert.deepEqual(WebAssembly.Module.imports(module),[{module:'env',name:'memory',kind:'memory'},...names.map(name=>({module:'ring3',name,kind:'function'}))]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]); count++;
}
assert.equal(count,8);
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn late_refusal_preserves_existing_owners_and_entire_arena() {
    let mut engine = code_at(PC, &[0x90, 0xeb, 0]);
    request(&mut engine, 3, false);
    let generation = engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    engine.map(0x3000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0xd9, 0x38, 0x0f, 0x0b]);
    engine.upload(0x3000, 4).unwrap();
    for resident in [false, true] {
        for entries in [false, true] {
            let descriptor = if entries {
                vec![0x3000_u32]
            } else {
                vec![0x3000_u32, 4]
            };
            for (index, value) in descriptor.iter().enumerate() {
                engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
            let before = (
                engine.arena().to_vec(),
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(id).unwrap().to_vec(),
            );
            assert!(match (resident, entries) {
                (false, false) => engine.compile(1).is_err(),
                (false, true) => engine.compile_entries(1, 0).is_err(),
                (true, false) => engine.compile_resident(1).is_err(),
                (true, true) => engine.compile_resident_entries(1, 0).is_err(),
            });
            assert_eq!(
                (
                    engine.arena().to_vec(),
                    engine.artifact_bytes().unwrap().to_vec(),
                    engine.resident_bytes(id).unwrap().to_vec()
                ),
                before
            );
            assert_eq!(engine.generation(), generation);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, id).unwrap();
        }
    }
}
