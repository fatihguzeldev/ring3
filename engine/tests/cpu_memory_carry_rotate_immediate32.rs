use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::decode::decode_one,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

fn code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    engine
}

#[test]
fn absolute_memory_dword_rcl_immediate_two_decodes() {
    let engine = code(&[0xc1, 0x15, 0x10, 0x40, 0, 0, 2]);
    let instruction = decode_one(engine.memory().unwrap(), GuestAddress(0x1000))
        .expect("absolute memory dword RCL with raw immediate two must decode");
    assert_eq!(instruction.length(), 7);
    assert_eq!(instruction.next_pc(), GuestAddress(0x1007));
}

#[test]
fn memory_dword_rcr_immediate_zero_compiles_without_operand_read() {
    let bytes = [0xc1, 0x1d, 0x10, 0x40, 0, 0, 0, 0xeb, 0];
    let mut engine = code(&bytes);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x4010), Access::Read)
            .is_err()
    );
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory dword RCR with raw immediate zero must compile without reading data");
}

use ring3_engine::{
    abi::{arena::HELPER_OFFSET, memory_helper::HELPER_SIZE},
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::DecodeError,
            ir::{EffectiveAddress, Operation, RotateKind},
        },
    },
    memory::{AddressSpace, FaultReason, MemoryFault},
    process::{HostError, ResidentInstallation},
};
use std::{
    collections::BTreeSet,
    io::Write,
    process::{Command, Stdio},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [RotateKind; 2] = [RotateKind::Left, RotateKind::Right];
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
const REPRESENTATIVE_RAW: [u8; 14] = [0, 1, 2, 31, 32, 33, 255, 0, 2, 31, 32, 33, 255, 1];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Replacement,
    Resident,
}

struct Shape {
    tail: Vec<u8>,
    address: EffectiveAddress,
}

fn ea(
    base: Option<Register32>,
    index: Option<Register32>,
    scale: u8,
    displacement: u32,
) -> EffectiveAddress {
    EffectiveAddress {
        base,
        index,
        scale,
        displacement,
    }
}

fn shapes() -> Vec<Shape> {
    let mut shapes = Vec::new();
    for (index, base) in REGISTERS.into_iter().enumerate() {
        let tail = match base {
            Register32::Esp => vec![0x04, 0x24],
            Register32::Ebp => vec![0x45, 0],
            _ => vec![index as u8],
        };
        shapes.push(Shape {
            tail,
            address: ea(Some(base), None, 1, 0),
        });
    }
    for (tail, address) in [
        (
            vec![0x40, 0x80],
            ea(Some(Register32::Eax), None, 1, 0xffff_ff80),
        ),
        (
            vec![0x82, 0x78, 0x56, 0x34, 0x12],
            ea(Some(Register32::Edx), None, 1, 0x1234_5678),
        ),
        (
            vec![0x44, 0x90, 0xe0],
            ea(Some(Register32::Eax), Some(Register32::Edx), 4, 0xffff_ffe0),
        ),
        (
            vec![0x84, 0xc2, 0x78, 0x56, 0x34, 0x12],
            ea(Some(Register32::Edx), Some(Register32::Eax), 8, 0x1234_5678),
        ),
        (
            vec![0x04, 0x85, 0x10, 0x50, 0, 0],
            ea(None, Some(Register32::Eax), 4, DATA + 0x10),
        ),
        (vec![0x05, 0x10, 0x50, 0, 0], ea(None, None, 1, DATA + 0x10)),
    ] {
        shapes.push(Shape { tail, address });
    }
    assert_eq!(shapes.len(), 14);
    shapes
}

fn instruction(opcode: u8, kind: RotateKind, shape: &Shape, raw: u8) -> Vec<u8> {
    assert_eq!(shape.tail[0] & 0x38, 0);
    let mut bytes = vec![opcode];
    bytes.extend_from_slice(&shape.tail);
    bytes[1] |= 0x10 | u8::from(kind == RotateKind::Right) << 3;
    if matches!(opcode, 0xc0 | 0xc1) {
        bytes.push(raw);
    }
    bytes
}

fn operation(kind: RotateKind, address: EffectiveAddress, raw: u8) -> Operation {
    if raw & 31 == 1 {
        Operation::MemoryRotateThroughCarryOne { kind, address }
    } else {
        Operation::MemoryRotateThroughCarryImmediate {
            kind,
            address,
            count: raw,
        }
    }
}

fn program(kind: RotateKind, shape: &Shape, raw: u8) -> Vec<u8> {
    let mut bytes = instruction(0xc1, kind, shape, raw);
    bytes.extend([0xeb, 0]);
    bytes
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fresh_code(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 3, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn spec(pc: u32, length: usize) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length as u32,
    }
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)], entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    for (index, &(pc, length)) in specs.iter().enumerate() {
        let offset = index * if entries { 4 } else { 8 };
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[offset + 4..offset + 8].copy_from_slice(&(length as u32).to_le_bytes());
        }
    }
}

fn compile(
    engine: &mut EngineInstance,
    owner: Owner,
    entries: bool,
    count: u32,
) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(count).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(count, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(count).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    }
}

fn module(engine: &EngineInstance, owner: Owner, id: u64) -> &[u8] {
    match owner {
        Owner::Replacement => engine.artifact_bytes().unwrap(),
        Owner::Resident => engine.resident_bytes(id).unwrap(),
    }
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
}

fn compile_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length,
    }
}

fn page(memory: &AddressSpace, pc: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    memory.read(GuestAddress(pc), &mut bytes).unwrap();
    bytes
}

fn validate_modules(modules: &[(Owner, Vec<u8>)]) {
    let mut input = Vec::new();
    for (owner, bytes) in modules {
        input.push(u8::from(*owner == Owner::Resident));
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
    }
    // validation only; no module instantiation or guest execution.
    let script = r#"
const assert = require('node:assert/strict');
const input = require('node:fs').readFileSync(0);
function cursor(bytes) {
  let at = 0;
  return {u() { let n=0, shift=0, b; do {assert.ok(at<bytes.length); b=bytes[at++];
    n+=(b&127)*2**shift; shift+=7; assert.ok(shift<=35);} while(b&128); return n; },
    byte() {assert.ok(at<bytes.length); return bytes[at++];},
    take(n) {assert.ok(at+n<=bytes.length); const out=bytes.subarray(at,at+n); at+=n; return out;},
    more() {return at<bytes.length;}, end() {assert.equal(at,bytes.length);}};
}
let offset=0, count=0;
while (offset<input.length) {
  assert.ok(offset+5<=input.length);
  const resident=input[offset++], length=input.readUInt32LE(offset); offset+=4;
  assert.ok(resident===0||resident===1); assert.ok(offset+length<=input.length);
  const bytes=input.subarray(offset,offset+length); offset+=length;
  assert.ok(WebAssembly.validate(bytes)); const module=new WebAssembly.Module(bytes);
  assert.deepEqual(WebAssembly.Module.imports(module), [
    {module:'env',name:'memory',kind:'memory'},
    {module:'ring3',name:resident?'guard_resident':'guard',kind:'function'},
    {module:'ring3',name:'read32',kind:'function'},
    {module:'ring3',name:resident?'store_resident32':'store32',kind:'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const sections=new Map(), file=cursor(bytes.subarray(8));
  while (file.more()) {const id=file.byte(), size=file.u(); sections.set(id,file.take(size));}
  file.end();
  const types=cursor(sections.get(1)); assert.equal(types.u(),4);
  for (const arity of [4,resident?7:6,1,resident?6:2]) {
    assert.equal(types.byte(),0x60); assert.equal(types.u(),arity);
    assert.deepEqual([...types.take(arity)],Array(arity).fill(0x7f));
    assert.equal(types.u(),1); assert.equal(types.byte(),0x7f);
  } types.end();
  const functions=cursor(sections.get(3)); assert.equal(functions.u(),1);
  assert.equal(functions.u(),0); functions.end();
  const bodies=cursor(sections.get(10)); assert.equal(bodies.u(),1);
  const body=cursor(bodies.take(bodies.u())); bodies.end(); assert.equal(body.u(),3);
  for (const [number,type] of [[16,0x7f],[1,0x7e],[6,0x7f]]) {
    assert.equal(body.u(),number); assert.equal(body.byte(),type);
  }
  count++;
}
process.stdout.write(String(count));
"#;
    let mut child = Command::new("node")
        .args(["-e", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, modules.len().to_string().as_bytes());
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    authority: (bool, u64),
    entries: [u64; 2],
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 3],
    mapped: u32,
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        authority: (engine.is_open(), engine.key()),
        entries: [KEEP, KEEP + 1].map(|pc| engine.lookup_resident(pc).unwrap().get()),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(id).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP, DATA].map(|pc| page(engine.memory().unwrap(), pc)),
        mapped: engine.memory().unwrap().mapped_pages(),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for pc in [CODE, KEEP, DATA] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, &[(KEEP, 3)], false);
    engine.compile(1).unwrap();
    describe(&mut engine, &[(KEEP, 3)], false);
    let id = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, id, 3)
        .unwrap();
    (engine, id)
}

fn assert_retained(engine: &EngineInstance, keep: u64, before: &Publication) {
    assert_eq!(publication(engine, keep), *before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert!(engine.lookup_resident(CODE).is_err());
}

fn assert_decoded(engine: &EngineInstance, pc: u32, bytes: &[u8], expected: Operation) {
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
    assert_eq!(decoded.pc(), GuestAddress(pc));
    assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
    assert_eq!(decoded.length() as usize, bytes.len());
    assert_eq!(
        decoded.next_pc(),
        GuestAddress(pc.wrapping_add(bytes.len() as u32))
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
}

#[test]
fn all_raw_counts_and_fourteen_addresses_keep_exact_ir_and_neighbors() {
    let mut engine = fresh_code(CODE, &[0x90], false);
    let all_shapes = shapes();
    let mut checked = 0;
    let mut raw_one = 0;
    let mut raw_general = 0;
    for kind in KINDS {
        for raw in 0..=u8::MAX {
            let shape = &all_shapes[13];
            let bytes = instruction(0xc1, kind, shape, raw);
            upload(&mut engine, CODE, &bytes);
            assert_decoded(&engine, CODE, &bytes, operation(kind, shape.address, raw));
            if raw & 31 == 1 {
                raw_one += 1;
            } else {
                raw_general += 1;
            }
            checked += 1;
        }
        for (shape, raw) in all_shapes.iter().zip(REPRESENTATIVE_RAW) {
            let bytes = instruction(0xc1, kind, shape, raw);
            upload(&mut engine, CODE, &bytes);
            assert_decoded(&engine, CODE, &bytes, operation(kind, shape.address, raw));
            checked += 1;
            let bytes = instruction(0xd1, kind, shape, 0);
            upload(&mut engine, CODE, &bytes);
            assert_decoded(
                &engine,
                CODE,
                &bytes,
                Operation::MemoryRotateThroughCarryOne {
                    kind,
                    address: shape.address,
                },
            );
            checked += 1;
        }
        let field = 0x10 | u8::from(kind == RotateKind::Right) << 3;
        for raw in [0, 1, 2, 31, 32, 33, 255] {
            let bytes = [0xc1, 0xc0 | field, raw];
            upload(&mut engine, CODE, &bytes);
            let expected = if raw & 31 == 1 {
                Operation::RotateThroughCarryOne {
                    kind,
                    destination: Register32::Eax,
                }
            } else {
                Operation::RotateThroughCarryImmediate {
                    kind,
                    destination: Register32::Eax,
                    count: raw,
                }
            };
            assert_decoded(&engine, CODE, &bytes, expected);
            checked += 1;
        }
        let bytes = [0xd3, 0xc1 | field];
        upload(&mut engine, CODE, &bytes);
        assert_decoded(
            &engine,
            CODE,
            &bytes,
            Operation::RotateThroughCarryCl {
                kind,
                destination: Register32::Ecx,
            },
        );
        checked += 1;
        let shape = &all_shapes[13];
        for opcode in [0xc0, 0xd2, 0xd0] {
            let bytes = instruction(opcode, kind, shape, 2);
            upload(&mut engine, CODE, &bytes);
            let expected = match opcode {
                0xc0 => Operation::MemoryByteRotateThroughCarryImmediate {
                    kind,
                    address: shape.address,
                    count: 2,
                },
                0xd2 => Operation::MemoryByteRotateThroughCarryCl {
                    kind,
                    address: shape.address,
                },
                0xd0 => Operation::MemoryByteRotateThroughCarryOne {
                    kind,
                    address: shape.address,
                },
                _ => unreachable!(),
            };
            assert_decoded(&engine, CODE, &bytes, expected);
            checked += 1;
        }
    }
    assert_eq!((raw_one, raw_general, checked), (16, 496, 590));
}

#[test]
fn memory_programs_require_four_bound_profiles_and_refuse_pure() {
    let mut modules = Vec::new();
    let mut pure = 0;
    let mut origins = [0; 3];
    for (shape, raw) in shapes().iter().zip(REPRESENTATIVE_RAW) {
        for kind in KINDS {
            let bytes = program(kind, shape, raw);
            let engine = fresh_code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            let rejected = error(CODE, InstructionError::BackendUnsupported);
            assert_eq!(
                compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default()).err(),
                Some(rejected)
            );
            assert_eq!(
                compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
                Some(rejected)
            );
            pure += 2;
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = fresh_code(CODE, &bytes, true);
                    let snapshot =
                        decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                    let mut before_code = vec![0; 4096];
                    engine
                        .memory()
                        .unwrap()
                        .fetch(GuestAddress(CODE), &mut before_code)
                        .unwrap();
                    describe(&mut engine, &[(CODE, bytes.len())], entries);
                    let before = engine.arena().to_vec();
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(DATA), Access::Read)
                            .is_err()
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(CODE), Access::Read)
                            .is_err()
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(snapshot.code_snapshot())
                    );
                    let mut after_code = vec![0; 4096];
                    engine
                        .memory()
                        .unwrap()
                        .fetch(GuestAddress(CODE), &mut after_code)
                        .unwrap();
                    assert_eq!(after_code, before_code);
                    guard(&engine, owner, id).unwrap();
                    if owner == Owner::Resident {
                        for pc in [CODE, CODE + bytes.len() as u32 - 2] {
                            assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                        }
                        for pc in CODE + 1..CODE + bytes.len() as u32 - 2 {
                            assert!(engine.lookup_resident(pc).is_err());
                        }
                        assert!(engine.lookup_resident(CODE + bytes.len() as u32).is_err());
                    }
                    origins[match raw & 31 {
                        0 => 0,
                        1 => 1,
                        _ => 2,
                    }] += 1;
                    modules.push((owner, module(&engine, owner, id).to_vec()));
                }
            }
        }
    }
    assert_eq!((modules.len(), pure, origins), (112, 56, [32, 32, 48]));
    validate_modules(&modules);
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = Vec::new();
    for kind in KINDS {
        for shape in [&shapes()[0], &shapes()[13]] {
            let bytes = instruction(0xc1, kind, shape, 2);
            for prefix in [
                0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
            ] {
                let cause = match prefix {
                    0xf0 => DecodeError::InvalidEncoding,
                    0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                        DecodeError::Unsupported(UnsupportedFeature::Segment)
                    }
                    _ => opcode,
                };
                cases.push(([vec![prefix], bytes.clone()].concat(), cause));
            }
            cases.push((
                [vec![0x66], instruction(0xd3, kind, shape, 0)].concat(),
                opcode,
            ));
            for byte_opcode in [0xc0, 0xd2, 0xd0] {
                cases.push((
                    [vec![0x66], instruction(byte_opcode, kind, shape, 2)].concat(),
                    opcode,
                ));
            }
            for plain_opcode in [0xc1, 0xd3] {
                let mut plain = instruction(plain_opcode, kind, shape, 2);
                plain[1] &= !0x10;
                cases.push(([vec![0x66], plain].concat(), opcode));
            }
        }
        let field = 0x10 | u8::from(kind == RotateKind::Right) << 3;
        cases.push((vec![0x66, 0x66, 0xc1, 0xc0 | field, 2], opcode));
        cases.push((vec![0x66, 0x66, 0xd3, 0xc0 | field], opcode));
    }
    for operand in [0xf0, 0x33] {
        cases.push((vec![0xc1, operand, 2], opcode));
    }
    assert_eq!(cases.len(), 74);
    assert_eq!(
        cases
            .iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        74
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, e)| *e == DecodeError::InvalidEncoding)
            .count(),
        4
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, e)| *e == DecodeError::Unsupported(UnsupportedFeature::Segment))
            .count(),
        24
    );
    assert_eq!(cases.iter().filter(|(_, e)| *e == opcode).count(), 46);
    cases
}

#[test]
fn strict_neighbors_and_late_failures_preserve_both_publications() {
    let positive = instruction(0xc1, RotateKind::Left, &shapes()[13], 2);
    let mut decodes = 0;
    let mut refusals = 0;
    for (excluded, cause) in exclusions() {
        let mut bytes = excluded.clone();
        bytes.extend([0xeb, 0]);
        let engine = fresh_code(CODE, &bytes, false);
        let memory = engine.memory().unwrap();
        let before = page(memory, CODE);
        assert_eq!(
            decode_one(memory, GuestAddress(CODE)).err(),
            Some(cause),
            "{excluded:02x?}"
        );
        decodes += 1;
        let failure = error(CODE, InstructionError::Decode(cause));
        assert_eq!(
            compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default()).err(),
            Some(failure)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(failure)
        );
        assert_eq!(page(memory, CODE), before);
        refusals += 2;
        let mut bytes = positive.clone();
        bytes.extend(excluded);
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &bytes);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let before = publication(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(
                        owner,
                        error(
                            CODE + positive.len() as u32,
                            InstructionError::Decode(cause)
                        )
                    ))
                );
                assert_retained(&engine, keep, &before);
                refusals += 1;
            }
        }
    }
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for kind in KINDS {
        let positive = instruction(0xc1, kind, &shapes()[13], 2);
        for (pc, tail, declared, failure) in [
            (
                CODE,
                vec![0x0f, 0x0b],
                positive.len() + 2,
                error(
                    CODE + positive.len() as u32,
                    InstructionError::Decode(opcode),
                ),
            ),
            (
                CODE,
                vec![0x66, 0xc1, 0x13, 2],
                positive.len() + 4,
                error(
                    CODE + positive.len() as u32,
                    InstructionError::Decode(opcode),
                ),
            ),
            (
                0x1ff7,
                vec![0xc1, 0x13],
                10,
                error(
                    0x1ffe,
                    InstructionError::Decode(fetch_error(0x1ffe, 0x2000, 3, FaultReason::Unmapped)),
                ),
            ),
        ] {
            let mut bytes = positive.clone();
            bytes.extend(tail);
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, keep) = prior_owners();
                    upload(&mut engine, pc, &bytes);
                    describe(&mut engine, &[(pc, declared)], entries);
                    let before = publication(&engine, keep);
                    assert_eq!(
                        compile(&mut engine, owner, entries, 1),
                        Err(compile_error(owner, failure))
                    );
                    assert_retained(&engine, keep, &before);
                    refusals += 1;
                }
            }
        }
    }
    assert_eq!((decodes, refusals), (74, 468));
}

fn helper_words(engine: &EngineInstance) -> [u32; 10] {
    assert_eq!(HELPER_SIZE, 40);
    std::array::from_fn(|index| {
        let offset = HELPER_OFFSET + index * 4;
        u32::from_le_bytes(engine.arena()[offset..offset + 4].try_into().unwrap())
    })
}

#[test]
fn exact_fetch_cuts_and_dword_endpoints_remain_distinct() {
    let (mut complete, mut truncated, mut permission) = (0, 0, 0);
    let mut encoded_bytes = 0;
    for (shape, raw) in shapes().iter().zip(REPRESENTATIVE_RAW) {
        for kind in KINDS {
            let target = instruction(0xc1, kind, shape, raw);
            encoded_bytes += target.len();
            let length = target.len() as u32;
            for pc in [0x2000 - length, u32::MAX - length + 1] {
                let engine = fresh_code(pc, &target, true);
                assert_decoded(&engine, pc, &target, operation(kind, shape.address, raw));
                complete += 1;
            }
            for present in 1..target.len() {
                let pc = 0x2000 - present as u32;
                let engine = fresh_code(pc, &target[..present], true);
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        0x2000,
                        present as u32 + 1,
                        FaultReason::Unmapped
                    ))
                );
                truncated += 1;
                let mut engine = fresh_code(pc, &target, false);
                engine.protect(0x2000, 1, 1).unwrap();
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(fetch_error(
                        pc,
                        0x2000,
                        present as u32 + 1,
                        FaultReason::Permission
                    ))
                );
                permission += 1;
            }
        }
    }
    assert_eq!(
        (encoded_bytes, complete, truncated, permission),
        (130, 56, 102, 102)
    );
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0xffff_f000, 1, 3).unwrap();
    engine.write32(0xffff_fffc, 0x9234_5678).unwrap();
    assert_eq!(
        helper_words(&engine),
        [
            u32::from_le_bytes(*b"R3MH"),
            0x0001_0001,
            40,
            0,
            0,
            0,
            0,
            0,
            0,
            0
        ]
    );
    engine.read32(0xffff_fffc).unwrap();
    assert_eq!(
        helper_words(&engine),
        [
            u32::from_le_bytes(*b"R3MH"),
            0x0001_0001,
            40,
            0,
            0,
            0x9234_5678,
            0,
            0,
            0,
            0
        ]
    );
    let mut endpoints = 2;
    for address in [0xffff_fffd, 0xffff_fffe, 0xffff_ffff] {
        engine.read32(address).unwrap();
        assert_eq!(
            helper_words(&engine),
            [
                u32::from_le_bytes(*b"R3MH"),
                0x0001_0001,
                40,
                0,
                1,
                0,
                3,
                address,
                1,
                4
            ]
        );
        engine.write32(address, 0).unwrap();
        assert_eq!(
            helper_words(&engine),
            [
                u32::from_le_bytes(*b"R3MH"),
                0x0001_0001,
                40,
                0,
                1,
                0,
                3,
                address,
                2,
                4
            ]
        );
        let mut current = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0xffff_fffc), &mut current)
            .unwrap();
        assert_eq!(current, 0x9234_5678_u32.to_le_bytes());
        endpoints += 2;
    }
    assert_eq!(endpoints, 8);
}

fn stale_error(owner: Owner) -> HostError {
    match owner {
        Owner::Replacement => HostError::CodeInvalidated,
        Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
    }
}

fn assert_stale_metadata(engine: &EngineInstance, owner: Owner, id: u64, generation: u32) {
    let arena = engine.arena().to_vec();
    assert_eq!(guard(engine, owner, id), Err(stale_error(owner)));
    match owner {
        Owner::Replacement => assert_eq!(engine.artifact_bytes(), Err(stale_error(owner))),
        Owner::Resident => assert_eq!(engine.resident_bytes(id), Err(stale_error(owner))),
    }
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.arena(), arena);
}

fn assert_prior_keep_current(
    engine: &EngineInstance,
    owner: Owner,
    keep: u64,
    generation: u32,
    saved: &[(Vec<u8>, usize); 2],
) {
    engine.guard_resident(KEY, keep).unwrap();
    let bytes = engine.resident_bytes(keep).unwrap();
    assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), &saved[1]);
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    let installed = engine.lookup_installed_resident(KEY, KEEP).unwrap();
    assert_eq!((installed.unit_id, installed.slot), (keep, 3));
    if owner == Owner::Resident {
        engine.guard(KEY, generation).unwrap();
        let bytes = engine.artifact_bytes().unwrap();
        assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), &saved[0]);
    }
}

#[test]
fn consumed_bytes_and_data_only_changes_preserve_currency_boundaries() {
    let mut mutations = 0;
    for kind in KINDS {
        let target = instruction(0xc1, kind, &shapes()[13], 2);
        let mut bytes = target.clone();
        bytes.extend([0xeb, 0]);
        for offset in 0..target.len() {
            for same in [true, false] {
                for owner in [Owner::Replacement, Owner::Resident] {
                    for entries in [false, true] {
                        let (mut engine, keep) = prior_owners();
                        let keep_generation = engine.generation();
                        let saved_keep = [
                            engine.artifact_bytes().unwrap(),
                            engine.resident_bytes(keep).unwrap(),
                        ]
                        .map(|module| (module.to_vec(), module.as_ptr() as usize));
                        upload(&mut engine, CODE, &bytes);
                        let snapshot = engine
                            .memory()
                            .unwrap()
                            .snapshot_code(GuestAddress(CODE), target.len())
                            .unwrap();
                        describe(&mut engine, &[(CODE, bytes.len())], entries);
                        let id = compile(&mut engine, owner, entries, 1).unwrap();
                        let saved_current = (
                            module(&engine, owner, id).to_vec(),
                            module(&engine, owner, id).as_ptr() as usize,
                        );
                        let generation = engine.generation();
                        guard(&engine, owner, id).unwrap();
                        assert_eq!(saved_current.0.len(), module(&engine, owner, id).len());
                        assert_prior_keep_current(
                            &engine,
                            owner,
                            keep,
                            keep_generation,
                            &saved_keep,
                        );
                        upload(
                            &mut engine,
                            CODE + offset as u32,
                            &[if same {
                                target[offset]
                            } else {
                                target[offset] ^ 1
                            }],
                        );
                        assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                        // stale public getters refuse; physical stale allocation bytes are an actual-wasm obligation.
                        assert_stale_metadata(&engine, owner, id, generation);
                        assert_prior_keep_current(
                            &engine,
                            owner,
                            keep,
                            keep_generation,
                            &saved_keep,
                        );
                        mutations += 1;
                    }
                }
            }
        }
        for (raw, alias) in [(0, 32), (2, 34)] {
            let target = instruction(0xc1, kind, &shapes()[13], raw);
            let bytes = program(kind, &shapes()[13], raw);
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, keep) = prior_owners();
                    let keep_generation = engine.generation();
                    let saved_keep = [
                        engine.artifact_bytes().unwrap(),
                        engine.resident_bytes(keep).unwrap(),
                    ]
                    .map(|module| (module.to_vec(), module.as_ptr() as usize));
                    upload(&mut engine, CODE, &bytes);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), target.len())
                        .unwrap();
                    describe(&mut engine, &[(CODE, bytes.len())], entries);
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    let generation = engine.generation();
                    let _saved_current = (
                        module(&engine, owner, id).to_vec(),
                        module(&engine, owner, id).as_ptr() as usize,
                    );
                    assert_eq!(raw & 31, alias & 31);
                    upload(&mut engine, CODE + target.len() as u32 - 1, &[alias]);
                    assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                    assert_stale_metadata(&engine, owner, id, generation);
                    assert_prior_keep_current(&engine, owner, keep, keep_generation, &saved_keep);
                    mutations += 1;
                }
            }
        }
    }
    assert_eq!(mutations, 128);
    let mut current_checks = 0;
    for kind in KINDS {
        let bytes = program(kind, &shapes()[13], 2);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                let keep_generation = engine.generation();
                let saved_keep = [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(keep).unwrap(),
                ]
                .map(|module| (module.to_vec(), module.as_ptr() as usize));
                upload(&mut engine, CODE, &bytes);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), bytes.len())
                    .unwrap();
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                let before = (
                    module(&engine, owner, id).to_vec(),
                    module(&engine, owner, id).as_ptr() as usize,
                );
                engine.write32(DATA + 0x10, 0x9234_5678).unwrap();
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(
                    (
                        module(&engine, owner, id).to_vec(),
                        module(&engine, owner, id).as_ptr() as usize
                    ),
                    before
                );
                guard(&engine, owner, id).unwrap();
                assert_prior_keep_current(&engine, owner, keep, keep_generation, &saved_keep);
                current_checks += 1;
                engine.map(0x7000, 1, 7).unwrap();
                upload(&mut engine, 0x7000, &[0x0f, 0x0b]);
                engine.protect(0x7000, 1, 4).unwrap();
                engine.unmap(0x7000, 1).unwrap();
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(
                    (
                        module(&engine, owner, id).to_vec(),
                        module(&engine, owner, id).as_ptr() as usize
                    ),
                    before
                );
                guard(&engine, owner, id).unwrap();
                assert_prior_keep_current(&engine, owner, keep, keep_generation, &saved_keep);
                current_checks += 1;
            }
        }
    }
    assert_eq!(current_checks, 16);
}

#[test]
fn default_limits_and_refusals_preserve_both_owner_publications() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut admitted = 0;
    let mut rejected = 0;
    for kind in KINDS {
        let target = instruction(0xc1, kind, &shapes()[13], 2);
        let mut positive = vec![0x90; 62];
        positive.extend(&target);
        positive.extend([0xeb, 0]);
        let mut negative = vec![0x90; 63];
        negative.extend(&target);
        negative.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &positive);
                describe(&mut engine, &[(CODE, positive.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                guard(&engine, owner, id).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                admitted += 1;
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &negative);
                describe(&mut engine, &[(CODE, negative.len())], entries);
                let before = publication(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(owner, CompileError::InstructionLimit))
                );
                assert_retained(&engine, keep, &before);
                rejected += 1;
            }
        }
    }
    assert_eq!((admitted, rejected), (8, 8));
    let (mut engine, keep) = prior_owners();
    let bytes = [0x90, 0xeb, 0];
    upload(&mut engine, CODE, &bytes);
    let before = publication(&engine, keep);
    let memory = engine.memory().unwrap();
    let actual = compile_region(memory, &[spec(CODE, bytes.len())], limits)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap()
        .len();
    let mut wasm_checks = 0;
    for limit in [actual, actual - 1, 1] {
        let tight = CompileLimits {
            wasm_bytes: limit,
            ..limits
        };
        for result in [
            compile_region(memory, &[spec(CODE, bytes.len())], tight),
            compile_entry_region(memory, &[GuestAddress(CODE)], tight),
        ] {
            if limit == actual {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.err(), Some(CompileError::WasmLimit));
            }
            wasm_checks += 1;
        }
        assert_retained(&engine, keep, &before);
    }
    let mut limit_checks = 0;
    for invalid in [
        CompileLimits {
            blocks: 0,
            ..limits
        },
        CompileLimits {
            blocks: 9,
            ..limits
        },
        CompileLimits {
            instructions: 0,
            ..limits
        },
        CompileLimits {
            instructions: 65,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 0,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 65_537,
            ..limits
        },
    ] {
        assert_eq!(
            compile_region(memory, &[spec(CODE, bytes.len())], invalid).err(),
            Some(CompileError::InvalidLimits)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], invalid).err(),
            Some(CompileError::InvalidLimits)
        );
        assert_retained(&engine, keep, &before);
        limit_checks += 2;
    }
    assert_eq!((wasm_checks, limit_checks), (6, 12));
    let atom = program(RotateKind::Left, &shapes()[13], 2);
    let bytes = atom.repeat(9);
    let mut blocks = 0;
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners();
            upload(&mut engine, CODE, &bytes);
            let specs: Vec<_> = (0..9)
                .map(|index| (CODE + index * atom.len() as u32, atom.len()))
                .collect();
            describe(&mut engine, &specs[..8], entries);
            let id = compile(&mut engine, owner, entries, 8).unwrap();
            let current = (
                module(&engine, owner, id).to_vec(),
                module(&engine, owner, id).as_ptr() as usize,
            );
            describe(&mut engine, &specs, entries);
            let expected = publication(&engine, keep);
            assert_eq!(
                compile(&mut engine, owner, entries, 9),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(publication(&engine, keep), expected);
            assert_eq!(
                (
                    module(&engine, owner, id).to_vec(),
                    module(&engine, owner, id).as_ptr() as usize
                ),
                current
            );
            guard(&engine, owner, id).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            blocks += 1;
        }
    }
    assert_eq!(blocks, 4);
}
