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
        .expect("unprefixed LODSB/STOSB must compile");
}

#[test]
fn lodsb_admits_in_bound_engine() {
    compile(0xac);
}

#[test]
fn stosb_admits_in_bound_engine() {
    compile(0xaa);
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

fn memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn exact_operations_fetch_without_data_and_wrap_the_next_pc() {
    for pc in [PC, 0x1fff, u32::MAX] {
        for (opcode, operation) in [
            (0xac, Operation::LoadStringByte),
            (0xaa, Operation::StoreStringByte),
        ] {
            let memory = memory(pc, &[opcode]);
            let instruction = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(instruction.operation(), &operation);
            assert_eq!(instruction.length(), 1);
            assert_eq!(instruction.next_pc(), GuestAddress(pc.wrapping_add(1)));
            assert!(memory.is_code_current(instruction.code_snapshot()));
        }
    }
}

#[test]
fn prefixes_wider_forms_and_standalone_memory_remain_excluded() {
    for opcode in [0xac, 0xaa] {
        for (prefix, feature) in [
            (0xf2, UnsupportedFeature::RepeatedString),
            (0xf3, UnsupportedFeature::RepeatedString),
            (0x64, UnsupportedFeature::Segment),
            (0x66, UnsupportedFeature::Opcode),
            (0x67, UnsupportedFeature::Opcode),
        ] {
            assert_eq!(
                decode_one(&memory(PC, &[prefix, opcode]), GuestAddress(PC)).unwrap_err(),
                DecodeError::Unsupported(feature)
            );
        }
        assert_eq!(
            decode_one(&memory(PC, &[0xf0, opcode]), GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let bytes = [0x90, opcode, 0xeb, 0];
        assert!(matches!(
            compile_region(
                &memory(PC, &bytes),
                &[BlockSpec {
                    entry: GuestAddress(PC),
                    byte_length: 4,
                }],
                CompileLimits::default()
            ),
            Err(CompileError::Instruction {
                pc: GuestAddress(0x1001),
                cause: InstructionError::BackendUnsupported,
            })
        ));
    }
    for bytes in [&[0x66, 0xa6][..], &[0x66, 0xae]] {
        assert_eq!(
            decode_one(&memory(PC, bytes), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        );
    }
}

#[test]
fn all_bound_compiler_profiles_validate_exact_helper_imports() {
    let mut input = Vec::new();
    for opcode in [0xac, 0xaa] {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
                engine.map(PC, 1, 7).unwrap();
                engine.arena_mut().unwrap()[140..143].copy_from_slice(&[opcode, 0xeb, 0]);
                engine.upload(PC, 3).unwrap();
                engine.protect(PC, 1, 4).unwrap();
                let descriptor = if entries {
                    PC.to_le_bytes().to_vec()
                } else {
                    [PC.to_le_bytes(), 3_u32.to_le_bytes()].concat()
                };
                engine.arena_mut().unwrap()[140..140 + descriptor.len()]
                    .copy_from_slice(&descriptor);
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
                input.extend([opcode, u8::from(resident)]);
                input.extend((bytes.len() as u32).to_le_bytes());
                input.extend(bytes);
            }
        }
    }
    let mut child = Command::new("node").args(["-e", r#"
const assert = require('node:assert/strict'), input = require('node:fs').readFileSync(0);
let at = 0, count = 0;
while (at < input.length) {
  const opcode = input[at++], resident = input[at++], length = input.readUInt32LE(at); at += 4;
  const bytes = input.subarray(at, at + length); at += length;
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes);
  const helper = opcode === 0xac ? 'read8' : resident ? 'store_resident8' : 'store8';
  assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env',name:'memory',kind:'memory'},
    ...[resident ? 'guard_resident' : 'guard', helper].map(name => ({module:'ring3',name,kind:'function'}))]);
  count++;
}
assert.equal(count, 8);
"#]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
