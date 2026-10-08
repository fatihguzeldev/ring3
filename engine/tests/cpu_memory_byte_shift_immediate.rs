use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

fn admits_memory_byte_shift_count_two(modrm: u8) {
    let bytes = [0xc0, modrm, 2, 0xeb, 0];
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory byte shift count two must admit with data unmapped");
}

#[test]
fn memory_shl_immediate_two_admits_with_unmapped_data() {
    admits_memory_byte_shift_count_two(0x23);
}

#[test]
fn memory_shr_immediate_two_admits_with_unmapped_data() {
    admits_memory_byte_shift_count_two(0x2b);
}

#[test]
fn memory_sar_immediate_two_admits_with_unmapped_data() {
    admits_memory_byte_shift_count_two(0x3b);
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
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, EffectiveAddress, Location32, Operation, ShiftCount, ShiftKind},
        },
    },
    memory::{AddressSpace, FaultReason, MemoryFault},
    process::{HostError, ResidentInstallation, StoreCompletion},
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const KINDS: [(ShiftKind, u8); 3] = [
    (ShiftKind::Shl, 4),
    (ShiftKind::Shr, 5),
    (ShiftKind::Sar, 7),
];
const RAW: [u8; 10] = [0, 1, 2, 7, 8, 9, 31, 32, 33, 255];

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
    vec![
        Shape {
            tail: vec![3],
            address: ea(Some(Register32::Ebx), None, 1, 0),
        },
        Shape {
            tail: vec![0x41, 0xf0],
            address: ea(Some(Register32::Ecx), None, 1, 0xffff_fff0),
        },
        Shape {
            tail: vec![4, 0x24],
            address: ea(Some(Register32::Esp), None, 1, 0),
        },
        Shape {
            tail: vec![0x45, 0],
            address: ea(Some(Register32::Ebp), None, 1, 0),
        },
        Shape {
            tail: vec![0x84, 0xcb, 0xe0, 0xff, 0xff, 0xff],
            address: ea(Some(Register32::Ebx), Some(Register32::Ecx), 8, 0xffff_ffe0),
        },
        Shape {
            tail: vec![5, 0x10, 0x50, 0, 0],
            address: ea(None, None, 1, DATA + 0x10),
        },
    ]
}

fn instruction(field: u8, shape: &Shape, raw: u8) -> Vec<u8> {
    assert_eq!(shape.tail[0] & 0x38, 0);
    let mut bytes = vec![0xc0];
    bytes.extend_from_slice(&shape.tail);
    bytes[1] |= field << 3;
    bytes.push(raw);
    bytes
}

fn operation(kind: ShiftKind, address: EffectiveAddress, raw: u8) -> Operation {
    if raw & 31 == 1 {
        Operation::MemoryShiftByte { kind, address }
    } else {
        Operation::MemoryShiftByteImmediate {
            kind,
            address,
            count: raw,
        }
    }
}

fn program(field: u8, shape: &Shape, raw: u8) -> Vec<u8> {
    let mut bytes = instruction(field, shape, raw);
    bytes.extend([0xeb, 0]);
    bytes
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fresh(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
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

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)], entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    for (index, &(pc, length)) in specs.iter().enumerate() {
        let at = index * if entries { 4 } else { 8 };
        transfer[at..at + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[at + 4..at + 8].copy_from_slice(&(length as u32).to_le_bytes());
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

fn host_error(owner: Owner, error: CompileError) -> HostError {
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

fn assert_decoded(engine: &EngineInstance, pc: u32, bytes: &[u8], expected: Operation) {
    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
    assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
    assert_eq!(
        (decoded.pc(), decoded.length() as usize, decoded.next_pc()),
        (
            GuestAddress(pc),
            bytes.len(),
            GuestAddress(pc.wrapping_add(bytes.len() as u32))
        )
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .is_code_current(decoded.code_snapshot())
    );
}

fn page(memory: &AddressSpace, address: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
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

fn validate_modules(modules: &[(Owner, Vec<u8>)]) {
    assert_eq!(modules.len(), 360);
    let mut input = Vec::new();
    for (owner, bytes) in modules {
        assert!(bytes.len() <= CompileLimits::default().wasm_bytes);
        input.push(u8::from(*owner == Owner::Resident));
        input.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        input.extend_from_slice(bytes);
    }
    // validation only; no instantiation or guest execution.
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
  assert.deepEqual(WebAssembly.Module.imports(module),[
    {module:'env',name:'memory',kind:'memory'},
    {module:'ring3',name:resident?'guard_resident':'guard',kind:'function'},
    {module:'ring3',name:'read8',kind:'function'},
    {module:'ring3',name:resident?'store_resident8':'store8',kind:'function'}]);
  assert.deepEqual(WebAssembly.Module.exports(module),[{name:'run',kind:'function'}]);
  const sections=new Map(), file=cursor(bytes.subarray(8));
  while (file.more()) {const id=file.byte(), size=file.u(); assert.ok(!sections.has(id)); sections.set(id,file.take(size));}
  file.end();
  const types=cursor(sections.get(1)); assert.equal(types.u(),4);
  for (const arity of [4,resident?7:6,1,resident?6:2]) {
    assert.equal(types.byte(),0x60); assert.equal(types.u(),arity);
    assert.deepEqual([...types.take(arity)],Array(arity).fill(0x7f));
    assert.equal(types.u(),1); assert.equal(types.byte(),0x7f);
  } types.end();
  const functions=cursor(sections.get(3)); assert.equal(functions.u(),1); assert.equal(functions.u(),0); functions.end();
  const bodies=cursor(sections.get(10)); assert.equal(bodies.u(),1);
  const body=cursor(bodies.take(bodies.u())); bodies.end(); assert.equal(body.u(),3);
  for (const [number,type] of [[16,0x7f],[1,0x7e],[6,0x7f]]) {assert.equal(body.u(),number); assert.equal(body.byte(),type);}
  count++;
}
assert.equal(offset,input.length); assert.equal(count,360); process.stdout.write(String(count));
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
    assert_eq!(output.stdout, b"360");
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 3],
    mapped: u32,
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, keep: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(keep).unwrap(),
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
    let keep = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, keep, 3)
        .unwrap();
    (engine, keep)
}

fn retained(engine: &EngineInstance, keep: u64, before: &Publication) {
    assert_eq!(publication(engine, keep), *before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert!(engine.lookup_resident(CODE).is_err());
}

#[test]
fn all_bare_raw_counts_and_six_eas_admit_bound_modules_without_operand_reads() {
    let mut engine = fresh(CODE, &[0x90], false);
    let mut raw_rows = 0;
    let mut old_one = 0;
    let mut new_immediate = 0;
    for (kind, field) in KINDS {
        for raw in 0..=u8::MAX {
            let bytes = instruction(field, &shapes()[0], raw);
            upload(&mut engine, CODE, &bytes);
            assert_decoded(
                &engine,
                CODE,
                &bytes,
                operation(kind, shapes()[0].address, raw),
            );
            raw_rows += 1;
            if raw & 31 == 1 {
                old_one += 1;
            } else {
                new_immediate += 1;
            }
        }
    }
    assert_eq!((raw_rows, old_one, new_immediate), (768, 24, 744));
    let mut ea_rows = 0;
    let mut pure_refusals = 0;
    let mut modules = Vec::new();
    for (kind, field) in KINDS {
        for shape in shapes() {
            for (index, raw) in RAW.into_iter().enumerate() {
                let target = instruction(field, &shape, raw);
                let bytes = program(field, &shape, raw);
                let entries = index % 2 == 1;
                let engine = fresh(CODE, &bytes, true);
                assert_decoded(&engine, CODE, &target, operation(kind, shape.address, raw));
                let before = engine.arena().to_vec();
                let expected = Some(error(CODE, InstructionError::BackendUnsupported));
                let observed = if entries {
                    compile_entry_region(
                        engine.memory().unwrap(),
                        &[GuestAddress(CODE)],
                        CompileLimits::default(),
                    )
                    .err()
                } else {
                    compile_region(
                        engine.memory().unwrap(),
                        &[BlockSpec {
                            entry: GuestAddress(CODE),
                            byte_length: bytes.len() as u32,
                        }],
                        CompileLimits::default(),
                    )
                    .err()
                };
                assert_eq!(observed, expected);
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), 0);
                assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                pure_refusals += 1;
                ea_rows += 1;
                for owner in [Owner::Replacement, Owner::Resident] {
                    let mut engine = fresh(CODE, &bytes, true);
                    seed_cpu(&mut engine, 0x8000_4102, 0xcd7);
                    describe(&mut engine, &[(CODE, bytes.len())], entries);
                    let before = engine.arena().to_vec();
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    for data in [0, DATA, u32::MAX] {
                        assert!(
                            engine
                                .memory()
                                .unwrap()
                                .resolve(GuestAddress(data), Access::Read)
                                .is_err()
                        );
                    }
                    guard(&engine, owner, id).unwrap();
                    if owner == Owner::Resident {
                        for pc in [CODE, CODE + target.len() as u32] {
                            assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                        }
                        for pc in CODE + 1..CODE + target.len() as u32 {
                            assert!(engine.lookup_resident(pc).is_err());
                        }
                        assert!(engine.lookup_resident(CODE + bytes.len() as u32).is_err());
                    }
                    modules.push((owner, module(&engine, owner, id).to_vec()));
                }
            }
        }
    }
    for (kind, field) in KINDS {
        let bytes = [0xd0, 3 | field << 3];
        upload(&mut engine, CODE, &bytes);
        assert_decoded(
            &engine,
            CODE,
            &bytes,
            Operation::MemoryShiftByte {
                kind,
                address: shapes()[0].address,
            },
        );
    }
    assert_eq!((ea_rows, pure_refusals, modules.len()), (180, 180, 360));
    validate_modules(&modules);
}

#[test]
fn strict_fetch_and_late_failures_keep_precise_categories_and_publications() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut strict = Vec::new();
    for (_, field) in KINDS {
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => unsupported,
            };
            strict.push((vec![prefix, 0xc0, 3 | field << 3, 2], expected));
        }
        strict.push((vec![0xd2, 3 | field << 3], unsupported));
        strict.push((vec![0x66, 0xc1, 3 | field << 3, 2], unsupported));
    }
    strict.push((vec![0xc0, 0x33, 2], unsupported));
    strict.push((vec![0xc0, 0xf0, 2], unsupported));
    for field in [0, 1, 2, 3] {
        strict.push((vec![0x66, 0xc0, 3 | field << 3, 2], unsupported));
    }
    assert_eq!(strict.len(), 45);
    let mut bound_failures = 0;
    for (bytes, expected) in strict {
        let engine = fresh(CODE, &bytes, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected)
        );
        for observed in [
            compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
            .err(),
            compile_entry_region(
                engine.memory().unwrap(),
                &[GuestAddress(CODE)],
                CompileLimits::default(),
            )
            .err(),
        ] {
            assert_eq!(
                observed,
                Some(error(CODE, InstructionError::Decode(expected)))
            );
        }
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &bytes);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let before = publication(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(host_error(
                        owner,
                        error(CODE, InstructionError::Decode(expected))
                    ))
                );
                retained(&engine, keep, &before);
                bound_failures += 1;
            }
        }
    }
    let (mut complete, mut unmapped, mut permission, mut overflow) = (0, 0, 0, 0);
    for (kind, field) in KINDS {
        for shape in shapes() {
            for raw in [0, 8] {
                let target = instruction(field, &shape, raw);
                for pc in [
                    0x2000 - target.len() as u32,
                    u32::MAX - target.len() as u32 + 1,
                ] {
                    let engine = fresh(pc, &target, true);
                    assert_decoded(&engine, pc, &target, operation(kind, shape.address, raw));
                    complete += 1;
                }
                for present in 0..target.len() {
                    let pc = 0x2000 - present as u32;
                    let mut engine = EngineInstance::new(2, KEY).unwrap();
                    engine.map(0x1000, 1, 7).unwrap();
                    if present != 0 {
                        upload(&mut engine, pc, &target[..present]);
                    }
                    engine.protect(0x1000, 1, 4).unwrap();
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
                    let mut engine = fresh(pc, &target, false);
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
                for present in 1..target.len() {
                    let pc = u32::MAX - present as u32 + 1;
                    let engine = fresh(pc, &target[..present], true);
                    assert_eq!(
                        decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                        Some(fetch_error(
                            pc,
                            pc,
                            present as u32 + 1,
                            FaultReason::AddressOverflow
                        ))
                    );
                    overflow += 1;
                }
            }
        }
    }
    let mut neighbors = 0;
    for (kind, field) in KINDS {
        for raw in [0, 2, 8] {
            let bytes = [0xc0, 0xc4 | field << 3, raw];
            let engine = fresh(CODE, &bytes, true);
            assert_decoded(
                &engine,
                CODE,
                &bytes,
                Operation::ShiftByte {
                    kind,
                    destination: ByteRegister::Ah,
                    count: ShiftCount::Immediate(raw),
                },
            );
            neighbors += 1;
        }
        let bytes = [0xc1, 3 | field << 3, 2];
        let engine = fresh(CODE, &bytes, true);
        assert_decoded(
            &engine,
            CODE,
            &bytes,
            Operation::Shift {
                kind,
                destination: Location32::Memory(shapes()[0].address),
                count: ShiftCount::Immediate(2),
            },
        );
        neighbors += 1;
    }
    let mut late = 0;
    for (_, field) in KINDS {
        let first = program(field, &shapes()[0], 2);
        for second in [vec![0x66, 0xc0, 3 | field << 3, 8], vec![0x0f, 0x0b]] {
            let mut bytes = first.clone();
            bytes.extend(&second);
            let next = CODE + first.len() as u32;
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, keep) = prior_owners();
                    upload(&mut engine, CODE, &bytes);
                    describe(
                        &mut engine,
                        &[(CODE, first.len()), (next, second.len())],
                        entries,
                    );
                    let before = publication(&engine, keep);
                    assert_eq!(
                        compile(&mut engine, owner, entries, 2),
                        Err(host_error(
                            owner,
                            error(next, InstructionError::Decode(unsupported))
                        ))
                    );
                    retained(&engine, keep, &before);
                    late += 1;
                }
            }
        }
    }
    assert_eq!(
        (
            bound_failures,
            complete,
            unmapped,
            permission,
            overflow,
            neighbors,
            late
        ),
        (180, 72, 180, 180, 144, 12, 24)
    );
}

fn check_helper(engine: &EngineInstance, before: &[u8], version: u16, fields: [u32; 6]) {
    assert_eq!(HELPER_SIZE, 40);
    let words = [
        u32::from_le_bytes(*b"R3MH"),
        0x0001_0000 | u32::from(version),
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
        Owner::Replacement => engine.store8(address, value),
        Owner::Resident => engine.store_resident8(KEY, id, address, value),
    }
    .unwrap()
}

#[test]
fn host_narrow_helper_faults_and_repairs_keep_width_one_and_whole_cpu() {
    // host helper observations only; no generated shift is executed.
    let (mut calls, mut faults, mut successes, mut repairs) = (0, 0, 0, 0);
    for owner in [Owner::Replacement, Owner::Resident] {
        for flags in [2_u32, 0xcd7] {
            for address in [DATA + 0x0f, u32::MAX] {
                for condition in 0..3 {
                    let bytes = program(4, &shapes()[0], 2);
                    let mut engine = fresh(CODE, &bytes, false);
                    describe(&mut engine, &[(CODE, bytes.len())], false);
                    let id = compile(&mut engine, owner, false, 1).unwrap();
                    let base = address & !0xfff;
                    let mut original: Vec<_> = (0..4096)
                        .map(|index| ((index * 13 + 7) & 255) as u8)
                        .collect();
                    original[(address - base) as usize] = 0x81;
                    if condition != 0 {
                        engine.map(base, 1, 3).unwrap();
                        upload(&mut engine, base, &original);
                        engine
                            .protect(base, 1, if condition == 1 { 2 } else { 1 })
                            .unwrap();
                    }
                    seed_cpu(&mut engine, 0x8000_4102, flags);
                    if condition < 2 {
                        for _ in 0..2 {
                            let before = engine.arena().to_vec();
                            engine.read8(address).unwrap();
                            check_helper(
                                &engine,
                                &before,
                                2,
                                [1, 0, if condition == 0 { 1 } else { 2 }, address, 1, 1],
                            );
                            guard(&engine, owner, id).unwrap();
                            calls += 1;
                            faults += 1;
                        }
                        if condition == 0 {
                            engine.map(base, 1, 3).unwrap();
                            original.fill(0);
                        } else {
                            engine.protect(base, 1, 3).unwrap();
                        }
                    } else {
                        let before = engine.arena().to_vec();
                        engine.read8(address).unwrap();
                        check_helper(&engine, &before, 2, [0, 0x81, 0, 0, 0, 1]);
                        calls += 1;
                        successes += 1;
                        for _ in 0..2 {
                            let before = engine.arena().to_vec();
                            assert_eq!(
                                owner_store(&mut engine, owner, id, address, 0x81),
                                StoreCompletion::Complete
                            );
                            check_helper(&engine, &before, 3, [1, 0, 2, address, 2, 1]);
                            assert_eq!(page(engine.memory().unwrap(), base), original);
                            guard(&engine, owner, id).unwrap();
                            calls += 1;
                            faults += 1;
                        }
                        engine.protect(base, 1, 3).unwrap();
                    }
                    repairs += 1;
                    assert_eq!(page(engine.memory().unwrap(), base), original);
                    let value = if condition == 0 { 0 } else { 0x81 };
                    let before = engine.arena().to_vec();
                    engine.read8(address).unwrap();
                    check_helper(&engine, &before, 2, [0, value, 0, 0, 0, 1]);
                    calls += 1;
                    successes += 1;
                    let before = engine.arena().to_vec();
                    assert_eq!(
                        owner_store(&mut engine, owner, id, address, value),
                        StoreCompletion::Complete
                    );
                    check_helper(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
                    calls += 1;
                    successes += 1;
                    assert_eq!(page(engine.memory().unwrap(), base), original);
                    guard(&engine, owner, id).unwrap();
                }
            }
        }
    }
    assert_eq!((calls, faults, successes, repairs), (104, 48, 56, 24));
}

struct Keep {
    id: u64,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
}

fn target_with_keep(
    field: u8,
    raw: u8,
    owner: Owner,
    entries: bool,
) -> (EngineInstance, u64, Keep) {
    let (mut engine, keep) = prior_owners();
    let kept = Keep {
        id: keep,
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(keep).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
    };
    let bytes = program(field, &shapes()[4], raw);
    upload(&mut engine, CODE, &bytes);
    describe(&mut engine, &[(CODE, bytes.len())], entries);
    let id = compile(&mut engine, owner, entries, 1).unwrap();
    (engine, id, kept)
}

fn keep_current(engine: &EngineInstance, owner: Owner, keep: &Keep) {
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

#[test]
fn cpu_inputs_and_consumed_source_writes_keep_distinct_owner_currency() {
    let (mut cpu_inputs, mut data_writes, mut mutations) = (0, 0, 0);
    for (_, field) in KINDS {
        for raw in [0, 1, 8] {
            let target = instruction(field, &shapes()[4], raw);
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, id, keep) = target_with_keep(field, raw, owner, entries);
                    let bytes = module(&engine, owner, id).to_vec();
                    let pointer = module(&engine, owner, id).as_ptr() as usize;
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), target.len())
                        .unwrap();
                    for ecx in [0_u32, 1, 255, 0x8000_4102] {
                        for flags in [2_u32, 0xcd7] {
                            seed_cpu(&mut engine, ecx, flags);
                            let before = engine.arena().to_vec();
                            guard(&engine, owner, id).unwrap();
                            assert_eq!(engine.arena(), before);
                            assert_eq!(
                                (
                                    module(&engine, owner, id).to_vec(),
                                    module(&engine, owner, id).as_ptr() as usize
                                ),
                                (bytes.clone(), pointer)
                            );
                            assert!(engine.memory().unwrap().is_code_current(&snapshot));
                            keep_current(&engine, owner, &keep);
                            cpu_inputs += 1;
                        }
                    }
                    let before = engine.arena().to_vec();
                    engine.write8(DATA + 0x10, 0x81).unwrap();
                    check_helper(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    guard(&engine, owner, id).unwrap();
                    keep_current(&engine, owner, &keep);
                    data_writes += 1;
                    for offset in 0..target.len() {
                        for changing in [false, true] {
                            let (mut engine, id, keep) =
                                target_with_keep(field, raw, owner, entries);
                            let snapshot = engine
                                .memory()
                                .unwrap()
                                .snapshot_code(GuestAddress(CODE), target.len())
                                .unwrap();
                            let generation = engine.generation();
                            let old = target[offset];
                            let value = if !changing {
                                old
                            } else if offset + 1 == target.len() {
                                old ^ 32
                            } else {
                                old ^ 1
                            };
                            let before = engine.arena().to_vec();
                            assert_eq!(
                                owner_store(
                                    &mut engine,
                                    owner,
                                    id,
                                    CODE + offset as u32,
                                    u32::from(value)
                                ),
                                StoreCompletion::CodeInvalidated
                            );
                            check_helper(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
                            assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                            let mut fetched = [0_u8];
                            engine
                                .memory()
                                .unwrap()
                                .fetch(GuestAddress(CODE + offset as u32), &mut fetched)
                                .unwrap();
                            assert_eq!(fetched, [value]);
                            let before = engine.arena().to_vec();
                            assert_eq!(guard(&engine, owner, id), Err(stale_error(owner)));
                            match owner {
                                Owner::Replacement => {
                                    assert_eq!(engine.artifact_bytes(), Err(stale_error(owner)))
                                }
                                Owner::Resident => {
                                    assert_eq!(engine.resident_bytes(id), Err(stale_error(owner)))
                                }
                            }
                            assert_eq!(engine.generation(), generation);
                            assert_eq!(engine.arena(), before);
                            keep_current(&engine, owner, &keep);
                            mutations += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!((cpu_inputs, data_writes, mutations), (288, 36, 576));
}

#[test]
fn default_instruction_and_block_caps_preserve_prior_publications() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let (mut admitted, mut rejected) = (0, 0);
    for (_, field) in KINDS {
        let target = instruction(field, &shapes()[5], 0);
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
                    Err(host_error(owner, CompileError::InstructionLimit))
                );
                retained(&engine, keep, &before);
                rejected += 1;
            }
        }
    }
    let atom = program(4, &shapes()[5], 0);
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
    assert_eq!((admitted, rejected), (16, 16));
}
