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
fn absolute_memory_dword_rcl_cl_decodes() {
    let engine = code(&[0xd3, 0x15, 0x10, 0x40, 0, 0]);
    let instruction = decode_one(engine.memory().unwrap(), GuestAddress(0x1000))
        .expect("absolute memory dword RCL with CL count must decode");
    assert_eq!(instruction.length(), 6);
    assert_eq!(instruction.next_pc(), GuestAddress(0x1006));
}

#[test]
fn memory_dword_rcr_cl_compiles_without_operand_read() {
    let bytes = [0xd3, 0x1d, 0x10, 0x40, 0, 0, 0xeb, 0];
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
        .expect("memory dword RCR with CL count must compile without reading data");
}

use ring3_engine::{
    abi::{
        arena::HELPER_OFFSET,
        memory_helper::HELPER_SIZE,
        x86::{EFLAGS_OFFSET, EIP_OFFSET, REGISTERS_OFFSET},
    },
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
    process::{HostError, ResidentInstallation, StoreCompletion},
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
const ECX_INPUTS: [u32; 8] = [
    0x4100,
    0x4101,
    0x4102,
    0x411f,
    0x4120,
    0x4121,
    0x41ff,
    0x8000_4102,
];

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
            vec![0x84, 0xca, 0x78, 0x56, 0x34, 0x12],
            ea(Some(Register32::Edx), Some(Register32::Ecx), 8, 0x1234_5678),
        ),
        (
            vec![0x04, 0x8d, 0x10, 0x50, 0, 0],
            ea(None, Some(Register32::Ecx), 4, DATA + 0x10),
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

fn operation(kind: RotateKind, address: EffectiveAddress) -> Operation {
    Operation::MemoryRotateThroughCarryCl { kind, address }
}

fn program(kind: RotateKind, shape: &Shape) -> Vec<u8> {
    let mut bytes = instruction(0xd3, kind, shape, 0);
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

fn seed_cpu(engine: &mut EngineInstance, ecx: u32, flags: u32) {
    for (index, value) in [
        0x1122_3344_u32,
        ecx,
        0x5566_7788,
        0x99aa_bbcc,
        0x4000,
        0xddee_ff01,
        0x1234_5678,
        0x8877_6655,
    ]
    .into_iter()
    .enumerate()
    {
        let at = REGISTERS_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    engine.arena_mut().unwrap()[EIP_OFFSET..EIP_OFFSET + 4].copy_from_slice(&CODE.to_le_bytes());
    engine.arena_mut().unwrap()[EFLAGS_OFFSET..EFLAGS_OFFSET + 4]
        .copy_from_slice(&flags.to_le_bytes());
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

use ring3_engine::cpu::x86::ir::ShiftCount;

#[test]
fn fourteen_memory_cl_addresses_preserve_exact_ir_and_neighbors() {
    let mut engine = fresh_code(CODE, &[0x90], false);
    let all_shapes = shapes();
    let mut targets = 0;
    let mut neighbors = 0;
    for kind in KINDS {
        for shape in &all_shapes {
            let bytes = instruction(0xd3, kind, shape, 0);
            upload(&mut engine, CODE, &bytes);
            assert_decoded(&engine, CODE, &bytes, operation(kind, shape.address));
            targets += 1;
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
            neighbors += 1;
        }
        let shape = &all_shapes[13];
        for raw in [0, 1, 2, 31, 32, 33, 255] {
            let bytes = instruction(0xc1, kind, shape, raw);
            upload(&mut engine, CODE, &bytes);
            let expected = if raw & 31 == 1 {
                Operation::MemoryRotateThroughCarryOne {
                    kind,
                    address: shape.address,
                }
            } else {
                Operation::MemoryRotateThroughCarryImmediate {
                    kind,
                    address: shape.address,
                    count: raw,
                }
            };
            assert_decoded(&engine, CODE, &bytes, expected);
            neighbors += 1;
        }
        let field = 0x10 | u8::from(kind == RotateKind::Right) << 3;
        for destination in [Register32::Eax, Register32::Ecx] {
            let bytes = [0xd3, 0xc0 | field | destination.index() as u8];
            upload(&mut engine, CODE, &bytes);
            assert_decoded(
                &engine,
                CODE,
                &bytes,
                Operation::RotateThroughCarryCl { kind, destination },
            );
            neighbors += 1;
        }
        for opcode in [0xc0, 0xd0, 0xd2] {
            let bytes = instruction(opcode, kind, shape, 2);
            upload(&mut engine, CODE, &bytes);
            let expected = match opcode {
                0xc0 => Operation::MemoryByteRotateThroughCarryImmediate {
                    kind,
                    address: shape.address,
                    count: 2,
                },
                0xd0 => Operation::MemoryByteRotateThroughCarryOne {
                    kind,
                    address: shape.address,
                },
                0xd2 => Operation::MemoryByteRotateThroughCarryCl {
                    kind,
                    address: shape.address,
                },
                _ => unreachable!(),
            };
            assert_decoded(&engine, CODE, &bytes, expected);
            neighbors += 1;
        }
        let mut bytes = instruction(0xd3, kind, shape, 0);
        bytes[1] &= !0x10;
        upload(&mut engine, CODE, &bytes);
        assert_decoded(
            &engine,
            CODE,
            &bytes,
            Operation::MemoryRotate {
                kind,
                address: shape.address,
                count: ShiftCount::Cl,
            },
        );
        neighbors += 1;
    }
    assert_eq!((targets, neighbors, targets + neighbors), (28, 54, 82));
}

#[test]
fn bound_memory_cl_profiles_and_producer_chains_validate_without_operand_reads() {
    let all_shapes = shapes();
    let mut programs = Vec::new();
    for shape in &all_shapes {
        for kind in KINDS {
            let target = instruction(0xd3, kind, shape, 0);
            programs.push((program(kind, shape), vec![0, target.len()], 0));
        }
    }
    for (shape_index, raw, producer, length) in
        [(1, 1, 0xf8, 9), (11, 0, 0xf9, 19), (4, 17, 0xf5, 11)]
    {
        let target = instruction(0xd3, RotateKind::Left, &all_shapes[shape_index], 0);
        let mut bytes = vec![0xb1, raw, producer];
        bytes.extend(&target);
        bytes.extend(&target);
        bytes.extend([0xeb, 0]);
        assert_eq!(bytes.len(), length);
        programs.push((
            bytes,
            vec![0, 2, 3, 3 + target.len(), 3 + target.len() * 2],
            3,
        ));
    }
    assert_eq!(programs.len(), 31);
    let mut modules = Vec::new();
    let mut pure = 0;
    for (index, (bytes, starts, first_memory)) in programs.iter().enumerate() {
        assert_eq!(starts.len(), if index < 28 { 2 } else { 5 });
        let engine = fresh_code(CODE, bytes, true);
        let memory = engine.memory().unwrap();
        let rejected = error(
            CODE + *first_memory as u32,
            InstructionError::BackendUnsupported,
        );
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
                let mut engine = fresh_code(CODE, bytes, true);
                seed_cpu(
                    &mut engine,
                    ECX_INPUTS[index % ECX_INPUTS.len()],
                    if index & 1 == 0 { 2 } else { 0xcd7 },
                );
                let mut before_code = vec![0; 4096];
                engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(CODE), &mut before_code)
                    .unwrap();
                let decoded: Vec<_> = starts
                    .iter()
                    .map(|at| {
                        decode_one(engine.memory().unwrap(), GuestAddress(CODE + *at as u32))
                            .unwrap()
                    })
                    .collect();
                for (position, decoded) in decoded.iter().enumerate() {
                    let next = starts.get(position + 1).copied().unwrap_or(bytes.len());
                    assert_eq!(decoded.length() as usize, next - starts[position]);
                    assert_eq!(decoded.next_pc(), GuestAddress(CODE + next as u32));
                }
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
                for decoded in &decoded {
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                }
                let mut after_code = vec![0; 4096];
                engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(CODE), &mut after_code)
                    .unwrap();
                assert_eq!(after_code, before_code);
                guard(&engine, owner, id).unwrap();
                if owner == Owner::Resident {
                    for at in 0..=bytes.len() {
                        if starts.contains(&at) {
                            assert_eq!(engine.lookup_resident(CODE + at as u32).unwrap().get(), id);
                        } else {
                            assert!(engine.lookup_resident(CODE + at as u32).is_err());
                        }
                    }
                }
                let bytes = module(&engine, owner, id);
                assert!(bytes.len() <= CompileLimits::default().wasm_bytes);
                modules.push((owner, bytes.to_vec()));
            }
        }
    }
    assert_eq!((modules.len(), pure), (124, 62));
    validate_modules(&modules);
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut rows = Vec::new();
    let all_shapes = shapes();
    for kind in KINDS {
        for shape in [&all_shapes[0], &all_shapes[11]] {
            let target = instruction(0xd3, kind, shape, 0);
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
                rows.push(([vec![prefix], target.clone()].concat(), cause));
            }
            for byte_opcode in [0xc0, 0xd0, 0xd2] {
                rows.push((
                    [vec![0x66], instruction(byte_opcode, kind, shape, 2)].concat(),
                    opcode,
                ));
            }
            for plain_opcode in [0xc1, 0xd3] {
                let mut plain = instruction(plain_opcode, kind, shape, 2);
                plain[1] &= !0x10;
                rows.push(([vec![0x66], plain].concat(), opcode));
            }
        }
        let field = 0x10 | u8::from(kind == RotateKind::Right) << 3;
        rows.push((vec![0x66, 0xc1, 0xc0 | field, 2], opcode));
        rows.push((vec![0x66, 0xd3, 0xc0 | field], opcode));
    }
    for operand in [0xf0, 0x33] {
        rows.push((vec![0xc1, operand, 2], opcode));
        rows.push((vec![0xd3, operand], opcode));
    }
    assert_eq!(rows.len(), 72);
    assert_eq!(
        rows.iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        72
    );
    assert_eq!(
        rows.iter()
            .filter(|(_, e)| *e == DecodeError::InvalidEncoding)
            .count(),
        4
    );
    assert_eq!(
        rows.iter()
            .filter(|(_, e)| *e == DecodeError::Unsupported(UnsupportedFeature::Segment))
            .count(),
        24
    );
    assert_eq!(rows.iter().filter(|(_, e)| *e == opcode).count(), 44);
    rows
}

#[test]
fn strict_and_late_memory_cl_failures_preserve_both_owner_publications() {
    let positive = instruction(0xd3, RotateKind::Left, &shapes()[13], 0);
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
        let positive = instruction(0xd3, kind, &shapes()[13], 0);
        for (pc, tail, declared, failure) in [
            (
                CODE,
                vec![0x0f, 0x0b],
                8,
                error(CODE + 6, InstructionError::Decode(opcode)),
            ),
            (
                CODE,
                vec![0x66, 0xd3, 0x10],
                9,
                error(CODE + 6, InstructionError::Decode(opcode)),
            ),
            (
                0x1ff8,
                vec![0xd3, 0x90],
                9,
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
    assert_eq!((decodes, refusals), (72, 456));
}

fn check_helper(engine: &EngineInstance, before: &[u8], fields: [u32; 6]) {
    assert_eq!(HELPER_SIZE, 40);
    let words = [
        u32::from_le_bytes(*b"R3MH"),
        0x0001_0001,
        40,
        0,
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        fields[4],
        fields[5],
    ];
    let mut expected = before.to_vec();
    for (index, word) in words.into_iter().enumerate() {
        let at = HELPER_OFFSET + index * 4;
        expected[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    assert_eq!(engine.arena(), expected);
}

fn owner_store(
    engine: &mut EngineInstance,
    owner: Owner,
    id: u64,
    address: u32,
    value: u32,
) -> StoreCompletion {
    match owner {
        Owner::Replacement => engine.store32(address, value),
        Owner::Resident => engine.store_resident32(KEY, id, address, value),
    }
    .unwrap()
}

fn fault_pages(
    engine: &mut EngineInstance,
    second_bits: Option<u32>,
) -> (Vec<u8>, Option<Vec<u8>>) {
    let first = page(engine.memory().unwrap(), 0x4000);
    let second = second_bits.map(|bits| {
        if bits & 1 == 0 {
            engine.protect(0x5000, 1, bits | 1).unwrap();
        }
        let saved = page(engine.memory().unwrap(), 0x5000);
        if bits & 1 == 0 {
            engine.protect(0x5000, 1, bits).unwrap();
        }
        saved
    });
    (first, second)
}

#[test]
fn exact_memory_cl_fetch_and_host_dword_atomic_repairs_remain_distinct() {
    let (mut complete, mut unmapped, mut permission, mut overflow, mut encoded) = (0, 0, 0, 0, 0);
    for shape in shapes() {
        for kind in KINDS {
            let target = instruction(0xd3, kind, &shape, 0);
            encoded += target.len();
            let length = target.len() as u32;
            for pc in [0x2000 - length, u32::MAX - length + 1] {
                let engine = fresh_code(pc, &target, true);
                assert_decoded(&engine, pc, &target, operation(kind, shape.address));
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
                unmapped += 1;
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
    for kind in KINDS {
        let target = instruction(0xd3, kind, &shapes()[13], 0);
        let engine = EngineInstance::new(1, KEY).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(fetch_error(CODE, CODE, 1, FaultReason::Unmapped))
        );
        unmapped += 1;
        let mut engine = fresh_code(CODE, &target, false);
        engine.protect(CODE, 1, 1).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(fetch_error(CODE, CODE, 1, FaultReason::Permission))
        );
        permission += 1;
        let engine = fresh_code(u32::MAX, &[0xd3], true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(u32::MAX)).err(),
            Some(fetch_error(
                u32::MAX,
                u32::MAX,
                2,
                FaultReason::AddressOverflow
            ))
        );
        overflow += 1;
    }
    assert_eq!(
        (encoded, complete, unmapped, permission, overflow),
        (102, 56, 76, 76, 2)
    );

    // these are host helper calls; no generated instruction is executed.
    let (mut calls, mut faults, mut successes, mut repairs) = (0, 0, 0, 0);
    for owner in [Owner::Replacement, Owner::Resident] {
        for flags in [2, 0xcd7] {
            for condition in 0..4 {
                let bytes = program(RotateKind::Left, &shapes()[13]);
                let mut engine = fresh_code(CODE, &bytes, false);
                describe(&mut engine, &[(CODE, bytes.len())], false);
                let id = compile(&mut engine, owner, false, 1).unwrap();
                engine.map(0x4000, 2, 3).unwrap();
                engine.write32(0x4ffe, 0x9234_5678).unwrap();
                let original = (
                    page(engine.memory().unwrap(), 0x4000),
                    page(engine.memory().unwrap(), 0x5000),
                );
                let second_bits = match condition {
                    0 => {
                        engine.unmap(0x5000, 1).unwrap();
                        None
                    }
                    1 => {
                        engine.protect(0x5000, 1, 2).unwrap();
                        Some(2)
                    }
                    2 => {
                        engine.protect(0x4000, 1, 1).unwrap();
                        Some(3)
                    }
                    3 => {
                        engine.protect(0x5000, 1, 1).unwrap();
                        Some(1)
                    }
                    _ => unreachable!(),
                };
                seed_cpu(&mut engine, 0x4100, flags);
                let expected_pages = (original.0.clone(), second_bits.map(|_| original.1.clone()));
                assert_eq!(fault_pages(&mut engine, second_bits), expected_pages);
                if condition >= 2 {
                    let before = engine.arena().to_vec();
                    engine.read32(0x4ffe).unwrap();
                    check_helper(&engine, &before, [0, 0x9234_5678, 0, 0, 0, 0]);
                    calls += 1;
                    successes += 1;
                }
                let detail = if condition == 0 { 1 } else { 2 };
                let address = if condition == 2 { 0x4ffe } else { 0x5000 };
                let access = if condition < 2 { 1 } else { 2 };
                for _ in 0..2 {
                    let before = engine.arena().to_vec();
                    if condition < 2 {
                        engine.read32(0x4ffe).unwrap();
                    } else {
                        assert_eq!(
                            owner_store(&mut engine, owner, id, 0x4ffe, 0x9234_5678),
                            StoreCompletion::Complete
                        );
                    }
                    check_helper(&engine, &before, [1, 0, detail, address, access, 4]);
                    assert_eq!(fault_pages(&mut engine, second_bits), expected_pages);
                    guard(&engine, owner, id).unwrap();
                    calls += 1;
                    faults += 1;
                }
                match condition {
                    0 => engine.map(0x5000, 1, 3).unwrap(),
                    1 | 3 => engine.protect(0x5000, 1, 3).unwrap(),
                    2 => engine.protect(0x4000, 1, 3).unwrap(),
                    _ => unreachable!(),
                }
                repairs += 1;
                let word = if condition == 0 { 0x5678 } else { 0x9234_5678 };
                let before = engine.arena().to_vec();
                engine.read32(0x4ffe).unwrap();
                check_helper(&engine, &before, [0, word, 0, 0, 0, 0]);
                calls += 1;
                successes += 1;
                let before = engine.arena().to_vec();
                assert_eq!(
                    owner_store(&mut engine, owner, id, 0x4ffe, word),
                    StoreCompletion::Complete
                );
                check_helper(&engine, &before, [0, 0, 0, 0, 0, 0]);
                calls += 1;
                successes += 1;
                let repaired_pages = (
                    original.0.clone(),
                    if condition == 0 {
                        vec![0; 4096]
                    } else {
                        original.1.clone()
                    },
                );
                assert_eq!(
                    (
                        page(engine.memory().unwrap(), 0x4000),
                        page(engine.memory().unwrap(), 0x5000)
                    ),
                    repaired_pages
                );
                guard(&engine, owner, id).unwrap();
            }
            let bytes = program(RotateKind::Right, &shapes()[13]);
            let mut engine = fresh_code(CODE, &bytes, false);
            describe(&mut engine, &[(CODE, bytes.len())], false);
            let id = compile(&mut engine, owner, false, 1).unwrap();
            engine.map(0xffff_f000, 1, 3).unwrap();
            engine.write32(0xffff_fffc, 0x9234_5678).unwrap();
            seed_cpu(&mut engine, 0x4120, flags);
            let original = page(engine.memory().unwrap(), 0xffff_f000);
            let before = engine.arena().to_vec();
            engine.read32(0xffff_fffc).unwrap();
            check_helper(&engine, &before, [0, 0x9234_5678, 0, 0, 0, 0]);
            calls += 1;
            successes += 1;
            let before = engine.arena().to_vec();
            assert_eq!(
                owner_store(&mut engine, owner, id, 0xffff_fffc, 0x9234_5678),
                StoreCompletion::Complete
            );
            check_helper(&engine, &before, [0, 0, 0, 0, 0, 0]);
            calls += 1;
            successes += 1;
            assert_eq!(page(engine.memory().unwrap(), 0xffff_f000), original);
            for address in [0xffff_fffd, 0xffff_fffe, 0xffff_ffff] {
                let before = engine.arena().to_vec();
                engine.read32(address).unwrap();
                check_helper(&engine, &before, [1, 0, 3, address, 1, 4]);
                calls += 1;
                faults += 1;
                let before = engine.arena().to_vec();
                assert_eq!(
                    owner_store(&mut engine, owner, id, address, 0),
                    StoreCompletion::Complete
                );
                check_helper(&engine, &before, [1, 0, 3, address, 2, 4]);
                calls += 1;
                faults += 1;
                assert_eq!(page(engine.memory().unwrap(), 0xffff_f000), original);
                guard(&engine, owner, id).unwrap();
            }
        }
    }
    assert_eq!((calls, faults, successes, repairs), (104, 56, 48, 16));
}

struct Keep {
    id: u64,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
}

fn target_with_keep(kind: RotateKind, owner: Owner, entries: bool) -> (EngineInstance, u64, Keep) {
    let (mut engine, keep) = prior_owners();
    let keep = Keep {
        id: keep,
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(keep).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
    };
    let bytes = program(kind, &shapes()[13]);
    upload(&mut engine, CODE, &bytes);
    seed_cpu(&mut engine, ECX_INPUTS[0], 2);
    describe(&mut engine, &[(CODE, bytes.len())], entries);
    let id = compile(&mut engine, owner, entries, 1).unwrap();
    (engine, id, keep)
}

fn assert_keep(engine: &EngineInstance, owner: Owner, keep: &Keep) {
    engine.guard_resident(KEY, keep.id).unwrap();
    let bytes = engine.resident_bytes(keep.id).unwrap();
    assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), &keep.modules[1]);
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep.id);
    let installed = engine.lookup_installed_resident(KEY, KEEP).unwrap();
    assert_eq!((installed.unit_id, installed.slot), (keep.id, 3));
    if owner == Owner::Resident {
        engine.guard(KEY, keep.generation).unwrap();
        let bytes = engine.artifact_bytes().unwrap();
        assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), &keep.modules[0]);
    }
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

#[test]
fn full_ecx_runtime_changes_and_consumed_writes_preserve_owner_boundaries() {
    let mut cpu_inputs = 0;
    let mut current_changes = 0;
    let mut mutations = 0;
    for kind in KINDS {
        let target = instruction(0xd3, kind, &shapes()[13], 0);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, id, keep) = target_with_keep(kind, owner, entries);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), target.len() + 2)
                    .unwrap();
                let current = (
                    module(&engine, owner, id).to_vec(),
                    module(&engine, owner, id).as_ptr() as usize,
                );
                let source = page(engine.memory().unwrap(), CODE);
                let identity = (engine.key(), engine.generation(), engine.arena_address());
                for ecx in ECX_INPUTS {
                    for flags in [2_u32, 0xcd7] {
                        let mut expected = engine.arena().to_vec();
                        let at = REGISTERS_OFFSET + Register32::Ecx.index() * 4;
                        expected[at..at + 4].copy_from_slice(&ecx.to_le_bytes());
                        expected[EFLAGS_OFFSET..EFLAGS_OFFSET + 4]
                            .copy_from_slice(&flags.to_le_bytes());
                        seed_cpu(&mut engine, ecx, flags);
                        assert_eq!(engine.arena(), expected);
                        assert_eq!(
                            (engine.key(), engine.generation(), engine.arena_address()),
                            identity
                        );
                        assert_eq!(page(engine.memory().unwrap(), CODE), source);
                        assert!(engine.memory().unwrap().is_code_current(&snapshot));
                        assert_eq!(
                            (
                                module(&engine, owner, id).to_vec(),
                                module(&engine, owner, id).as_ptr() as usize
                            ),
                            current
                        );
                        guard(&engine, owner, id).unwrap();
                        assert_keep(&engine, owner, &keep);
                        cpu_inputs += 1;
                    }
                }
                engine.write32(DATA + 0x10, 0x9234_5678).unwrap();
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(
                    (
                        module(&engine, owner, id).to_vec(),
                        module(&engine, owner, id).as_ptr() as usize
                    ),
                    current
                );
                guard(&engine, owner, id).unwrap();
                assert_keep(&engine, owner, &keep);
                current_changes += 1;
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
                    current
                );
                assert_eq!(page(engine.memory().unwrap(), CODE), source);
                guard(&engine, owner, id).unwrap();
                assert_keep(&engine, owner, &keep);
                current_changes += 1;
            }
        }
        for offset in 0..target.len() {
            for same in [true, false] {
                for owner in [Owner::Replacement, Owner::Resident] {
                    for entries in [false, true] {
                        let (mut engine, id, keep) = target_with_keep(kind, owner, entries);
                        let snapshot = engine
                            .memory()
                            .unwrap()
                            .snapshot_code(GuestAddress(CODE), target.len())
                            .unwrap();
                        let saved = (
                            module(&engine, owner, id).to_vec(),
                            module(&engine, owner, id).as_ptr() as usize,
                        );
                        assert!(saved.0.len() > 8);
                        guard(&engine, owner, id).unwrap();
                        assert_keep(&engine, owner, &keep);
                        let generation = engine.generation();
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
                        // the known arena is post-upload; stale allocation bytes are an actual-wasm obligation.
                        assert_stale_metadata(&engine, owner, id, generation);
                        assert_keep(&engine, owner, &keep);
                        mutations += 1;
                    }
                }
            }
        }
        for same in [true, false] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, id, keep) = target_with_keep(kind, owner, entries);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), target.len())
                        .unwrap();
                    let saved = (
                        module(&engine, owner, id).to_vec(),
                        module(&engine, owner, id).as_ptr() as usize,
                    );
                    assert!(saved.0.len() > 8);
                    let generation = engine.generation();
                    let old = u32::from_le_bytes(target[2..6].try_into().unwrap());
                    let before = engine.arena().to_vec();
                    assert_eq!(
                        owner_store(
                            &mut engine,
                            owner,
                            id,
                            CODE + 2,
                            if same { old } else { old ^ 1 }
                        ),
                        StoreCompletion::CodeInvalidated
                    );
                    check_helper(&engine, &before, [0, 0, 0, 0, 0, 0]);
                    assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                    // the known arena is post-helper; no stale allocation dereference is performed.
                    assert_stale_metadata(&engine, owner, id, generation);
                    assert_keep(&engine, owner, &keep);
                    mutations += 1;
                }
            }
        }
    }
    assert_eq!((cpu_inputs, current_changes, mutations), (128, 16, 112));
}

#[test]
fn default_memory_cl_instruction_and_block_caps_retain_prior_publications() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let (mut admitted, mut rejected) = (0, 0);
    for kind in KINDS {
        let target = instruction(0xd3, kind, &shapes()[13], 0);
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
    let atom = program(RotateKind::Left, &shapes()[13]);
    let bytes = atom.repeat(9);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners();
            upload(&mut engine, CODE, &bytes);
            let specs: Vec<_> = (0..9)
                .map(|index| (CODE + index * atom.len() as u32, atom.len()))
                .collect();
            describe(&mut engine, &specs[..8], entries);
            let id = compile(&mut engine, owner, entries, 8).unwrap();
            guard(&engine, owner, id).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            admitted += 1;
            let current = (
                module(&engine, owner, id).to_vec(),
                module(&engine, owner, id).as_ptr() as usize,
            );
            describe(&mut engine, &specs, entries);
            let before = publication(&engine, keep);
            assert_eq!(
                compile(&mut engine, owner, entries, 9),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(publication(&engine, keep), before);
            assert_eq!(
                (
                    module(&engine, owner, id).to_vec(),
                    module(&engine, owner, id).as_ptr() as usize
                ),
                current
            );
            guard(&engine, owner, id).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            rejected += 1;
        }
    }
    assert_eq!((admitted, rejected), (12, 12));
}
