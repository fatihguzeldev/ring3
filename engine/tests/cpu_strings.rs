use ring3_engine::process::EngineInstance;

const PC: u32 = 0x1000;

fn compile(instruction: u8) {
    let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[instruction, 0xeb, 0]);
    engine.upload(PC, 3).unwrap();
    engine.protect(PC, 1, 4).unwrap();
    let descriptor = &mut engine.arena_mut().unwrap()[140..148];
    descriptor[..4].copy_from_slice(&PC.to_le_bytes());
    descriptor[4..].copy_from_slice(&3_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("unprefixed string lane must compile");
}

#[test]
fn movsb_admits_in_bound_engine() {
    compile(0xa4);
}

#[test]
fn cld_admits_in_bound_engine() {
    compile(0xfc);
}

#[test]
fn std_admits_in_bound_engine() {
    compile(0xfd);
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

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(PC), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(PC), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    memory
}

#[test]
fn string_lane_decodes_exact_operations_without_data_mappings() {
    for (byte, operation) in [
        (0xa4, Operation::MoveStringByte),
        (0xfc, Operation::Direction { set: false }),
        (0xfd, Operation::Direction { set: true }),
    ] {
        let memory = memory(&[byte]);
        let decoded = decode_one(&memory, GuestAddress(PC)).unwrap();
        assert_eq!(decoded.operation(), &operation);
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (1, GuestAddress(PC + 1))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn repeated_prefixed_and_wider_strings_remain_excluded() {
    for (bytes, feature) in [
        (vec![0xf3, 0xa4], UnsupportedFeature::RepeatedString),
        (vec![0xf2, 0xa4], UnsupportedFeature::RepeatedString),
        (vec![0x64, 0xa4], UnsupportedFeature::Segment),
        (vec![0x66, 0xa4], UnsupportedFeature::Opcode),
        (vec![0x67, 0xa4], UnsupportedFeature::Opcode),
        (vec![0xa5], UnsupportedFeature::Opcode),
        (vec![0xaa], UnsupportedFeature::Opcode),
    ] {
        assert_eq!(
            decode_one(&memory(&bytes), GuestAddress(PC)).unwrap_err(),
            DecodeError::Unsupported(feature),
            "{bytes:02x?}"
        );
    }
    assert_eq!(
        decode_one(&memory(&[0xf0, 0xa4]), GuestAddress(PC)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
}

#[test]
fn bound_string_modules_validate_and_standalone_movsb_is_refused() {
    let bytes = [0xfd, 0xa4, 0xfc, 0xa4, 0xeb, 0];
    let memory = memory(&bytes);
    assert!(matches!(
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: 6
            }],
            CompileLimits::default()
        ),
        Err(CompileError::Instruction {
            pc: GuestAddress(0x1001),
            cause: InstructionError::BackendUnsupported
        })
    ));
    let mut modules = Vec::new();
    for resident in [false, true] {
        let mut engine = EngineInstance::new(1, 0x5152_5354_5556_5758).unwrap();
        engine.map(PC, 1, 7).unwrap();
        engine.arena_mut().unwrap()[140..146].copy_from_slice(&bytes);
        engine.upload(PC, 6).unwrap();
        engine.protect(PC, 1, 4).unwrap();
        engine.arena_mut().unwrap()[140..148]
            .copy_from_slice(&[PC.to_le_bytes(), 6_u32.to_le_bytes()].concat());
        let module = if resident {
            let id = engine.compile_resident(1).unwrap();
            engine.resident_bytes(id.get()).unwrap().to_vec()
        } else {
            engine.compile(1).unwrap();
            engine.artifact_bytes().unwrap().to_vec()
        };
        modules.push((resident, module));
    }
    let mut input = Vec::new();
    for (resident, module) in modules {
        input.push(u8::from(resident));
        input.extend((module.len() as u32).to_le_bytes());
        input.extend(module);
    }
    node(
        r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
let at = 0, count = 0;
while (at < input.length) {
  const resident = input[at++], length = input.readUInt32LE(at); at += 4;
  const bytes = input.subarray(at, at + length); at += length;
  assert.ok(WebAssembly.validate(bytes));
  const module = new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module:'env',name:'memory',kind:'memory'},
    ...[resident ? 'guard_resident' : 'guard', 'read8', resident ? 'store_resident8' : 'store8']
      .map(name => ({module:'ring3',name,kind:'function'}))]);
  count++;
}
assert.equal(count, 2);
"#,
        &input,
    );
}

fn node(script: &str, input: &[u8]) {
    let mut child = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn generated_standalone_cld_std_preserve_every_other_cpu_field() {
    let memory = memory(&[0xfc, 0xfd, 0xeb, 0]);
    let artifact = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(PC),
            byte_length: 4,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    node(
        r#"
const assert = require('node:assert/strict');
const module = new WebAssembly.Module(require('node:fs').readFileSync(0));
assert.deepEqual(WebAssembly.Module.imports(module), [{module:'env',name:'memory',kind:'memory'}]);
const memory = new WebAssembly.Memory({initial:1}), bytes = new Uint8Array(memory.buffer), view = new DataView(memory.buffer);
const run = new WebAssembly.Instance(module, {env:{memory}}).exports.run;
function header(at, magic, size) {
  bytes.set(Buffer.from(magic), at); view.setUint16(at+4,1,true); view.setUint16(at+6,1,true); view.setUint32(at+8,size,true);
}
for (const flags of [2,0xcd7]) {
  bytes.fill(0); header(0,'R3ST',56); header(56,'R3EX',40);
  const registers = [0x12345678,0x89abcdef,0x33445566,0x778899aa,0xaabbccdd,0x11223344,0xfffffffe,0xffffffff];
  registers.forEach((value,i) => view.setUint32(16+i*4,value,true));
  view.setUint32(48,0x1000,true); view.setUint32(52,flags,true);
  assert.equal(run(0,56,1,96),0); assert.equal(view.getUint32(48,true),0x1001);
  assert.equal(view.getUint32(52,true),flags & ~0x400); assert.equal(view.getUint32(76,true),1);
  assert.equal(run(0,56,1,96),0); assert.equal(view.getUint32(48,true),0x1002);
  assert.equal(view.getUint32(52,true),flags | 0x400); assert.equal(view.getUint32(76,true),1);
  assert.deepEqual(Array.from({length:8},(_,i)=>view.getUint32(16+i*4,true)),registers);
}
    "#,
        artifact.wasm_bytes(&memory).unwrap(),
    );
}

#[test]
fn single_byte_fetch_and_existing_instruction_caps_remain_exact() {
    for pc in [0x1fff_u32, 0xffff_ffff] {
        for opcode in [0xa4, 0xfc, 0xfd] {
            let mut memory = AddressSpace::new(1).unwrap();
            let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
            memory.map_zeroed(page, Permissions::ALL).unwrap();
            memory.write(GuestAddress(pc), &[opcode]).unwrap();
            memory.protect(page, Permissions::EXECUTE).unwrap();
            let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length(), 1);
            assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(1)));
        }
    }
    let memory = memory(&[0xfc, 0xfd, 0xeb, 0]);
    let block = [BlockSpec {
        entry: GuestAddress(PC),
        byte_length: 4,
    }];
    assert_eq!(
        compile_region(
            &memory,
            &block,
            CompileLimits {
                instructions: 2,
                ..CompileLimits::default()
            }
        )
        .unwrap_err(),
        CompileError::InstructionLimit
    );
    let artifact = compile_region(
        &memory,
        &block,
        CompileLimits {
            instructions: 3,
            ..CompileLimits::default()
        },
    )
    .unwrap();
    assert_eq!(artifact.metadata().instructions, 3);
}
