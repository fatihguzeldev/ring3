use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

const PC: u32 = 0x1000;
const KEY: u64 = 0x726f_6c61_6263_6465;

fn compiled(instruction: &[u8]) -> EngineInstance {
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
    engine
        .compile(1)
        .expect("rol must compile through the public api");
    engine
}

#[test]
fn word_rol_one_admits_in_bound_engine() {
    compiled(&[0x66, 0xd1, 0xc0]);
}

#[test]
fn word_rol_immediate_admits_in_bound_engine() {
    compiled(&[0x66, 0xc1, 0xc0, 0xff]);
}

#[test]
fn word_rol_cl_admits_in_bound_engine() {
    compiled(&[0x66, 0xd3, 0xc0]);
}

#[test]
fn old_rol_paths_validate_and_optionally_capture_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_ROL_MODULE_OUTPUT_DIR").map(PathBuf::from);
    let cases: [(&str, &[u8]); 6] = [
        ("byte-rol-one", &[0xd0, 0xc0]),
        ("byte-rol-immediate", &[0xc0, 0xc0, 0xff]),
        ("byte-rol-cl", &[0xd2, 0xc0]),
        ("dword-rol-one", &[0xd1, 0xc0]),
        ("dword-rol-immediate", &[0xc1, 0xc0, 0xff]),
        ("dword-rol-cl", &[0xd3, 0xc0]),
    ];
    if let Some(directory) = &directory {
        assert!(directory.is_dir() && !directory.is_symlink());
        for (name, _) in cases {
            assert!(!directory.join(format!("{name}.wasm")).exists());
        }
    }
    for (name, instruction) in cases {
        let engine = compiled(instruction);
        let bytes = engine.artifact_bytes().unwrap();
        let mut child = Command::new("node")
            .args([
                "-e",
                r#"
const assert = require('node:assert/strict');
const bytes = require('node:fs').readFileSync(0);
assert.ok(WebAssembly.validate(bytes));
const module = new WebAssembly.Module(bytes);
assert.deepEqual(WebAssembly.Module.imports(module), [
    { module: 'env', name: 'memory', kind: 'memory' },
    { module: 'ring3', name: 'guard', kind: 'function' },
]);
assert.deepEqual(WebAssembly.Module.exports(module), [
    { name: 'run', kind: 'function' },
]);
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

use iced_x86::{Code, Decoder, DecoderOptions};
use ring3_engine::{
    abi::arena::TRANSFER_SIZE,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Operation, RotateKind, ShiftCount},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const PARENTS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];
const FORMS: [&[u8]; 3] = [
    &[0x66, 0xd1, 0xc0],
    &[0x66, 0xc1, 0xc0, 0xff],
    &[0x66, 0xd3, 0xc0],
];

fn encoding(opcode: u8, modrm: u8, raw: u8) -> Vec<u8> {
    let mut bytes = vec![0x66, opcode, modrm];
    if opcode == 0xc1 {
        bytes.push(raw);
    }
    bytes
}

fn forms() -> Vec<(Vec<u8>, Code, Operation)> {
    let mut result = Vec::new();
    for (opcode, code, count) in [
        (0xd1, Code::Rol_rm16_1, ShiftCount::Immediate(1)),
        (0xc1, Code::Rol_rm16_imm8, ShiftCount::Immediate(255)),
        (0xd3, Code::Rol_rm16_CL, ShiftCount::Cl),
    ] {
        for (index, destination) in PARENTS.into_iter().enumerate() {
            result.push((
                encoding(opcode, 0xc0 | index as u8, 255),
                code,
                Operation::RotateWord {
                    kind: RotateKind::Left,
                    destination,
                    count,
                },
            ));
        }
    }
    result
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, bytes: usize, entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(bytes as u32).to_le_bytes());
    }
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
fn word_rol_types_all_parents_and_keeps_every_immediate_byte_literal() {
    let typed = forms();
    assert_eq!(typed.len(), 24);
    let mut engine = fixture(PC, &[0x90]);
    for (bytes, code, expected) in typed {
        upload(&mut engine, PC, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
        assert_eq!(
            Decoder::with_ip(32, &bytes, u64::from(PC), DecoderOptions::NONE)
                .decode()
                .code(),
            code
        );
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (bytes.len() as u8, GuestAddress(PC + bytes.len() as u32))
        );
    }
    for raw in 0..=u8::MAX {
        let bytes = encoding(0xc1, 0xc1, raw);
        upload(&mut engine, PC, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::RotateWord {
                kind: RotateKind::Left,
                destination: Register32::Ecx,
                count: ShiftCount::Immediate(raw),
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (4, GuestAddress(PC + 4))
        );
    }
}

#[test]
fn word_rol_strict_fetch_top_and_consumed_currency_stay_exact() {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for instruction in FORMS {
        for prefix in [0x66, 0x67, 0xf2, 0xf3, 0x64, 0xf0] {
            let bytes = [vec![prefix], instruction.to_vec()].concat();
            let expected = match prefix {
                0x64 => DecodeError::Unsupported(UnsupportedFeature::Segment),
                0xf0 => DecodeError::InvalidEncoding,
                _ => unsupported,
            };
            assert_eq!(
                decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
                expected,
                "{bytes:02x?}"
            );
        }
        let mut memory = instruction.to_vec();
        memory[2] &= 0x3f;
        memory.insert(0, 0x66);
        assert_eq!(
            decode_one(fixture(PC, &memory).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
            unsupported
        );
        let mut byte = instruction.to_vec();
        byte[1] -= 1;
        assert_eq!(
            decode_one(fixture(PC, &byte).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
            unsupported
        );
        for cut in 1..instruction.len() {
            let pc = 0x2000 - cut as u32;
            assert_eq!(
                decode_one(
                    fixture(pc, &instruction[..cut]).memory().unwrap(),
                    GuestAddress(pc)
                )
                .unwrap_err(),
                DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped,
                    },
                    length: cut as u32 + 1
                }
            );
        }
        let top = u32::MAX - (instruction.len() - 1) as u32;
        assert_eq!(
            decode_one(
                fixture(top, instruction).memory().unwrap(),
                GuestAddress(top)
            )
            .unwrap()
            .next_pc(),
            GuestAddress(0)
        );
        for (offset, original) in instruction.iter().copied().enumerate() {
            for changing in [false, true] {
                let mut engine = fixture(PC, instruction);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(PC)).unwrap();
                upload(
                    &mut engine,
                    PC + offset as u32,
                    &[original ^ u8::from(changing)],
                );
                assert!(
                    !engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
            }
        }
    }
    for opcode in [0xd1, 0xc1, 0xd3] {
        for extension in [2, 3] {
            let mut bytes = encoding(opcode, 0xc0 | extension << 3, 1);
            bytes.insert(0, 0x66);
            assert_eq!(
                decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
                unsupported
            );
        }
    }
    for bytes in [
        vec![0x66, 0x66, 0x0f, 0xa4, 0xc0, 1],
        vec![0x66, 0x66, 0x0f, 0xa5, 0xc0],
        vec![0x66, 0x66, 0x0f, 0xac, 0xc0, 1],
        vec![0x66, 0x66, 0x0f, 0xad, 0xc0],
    ] {
        assert_eq!(
            decode_one(fixture(PC, &bytes).memory().unwrap(), GuestAddress(PC)).unwrap_err(),
            unsupported
        );
    }
}

fn validate_profile(bytes: &[u8], guard: &str) {
    let mut child = Command::new("node")
        .args([
            "-e",
            r#"
const assert = require('node:assert/strict');
const bytes = require('node:fs').readFileSync(0);
const guard = process.argv[1];
const module = new WebAssembly.Module(bytes);
const imports = [{ module: 'env', name: 'memory', kind: 'memory' }];
if (guard) imports.push({ module: 'ring3', name: guard, kind: 'function' });
assert.deepEqual(WebAssembly.Module.imports(module), imports);
assert.deepEqual(WebAssembly.Module.exports(module), [{ name: 'run', kind: 'function' }]);
let at = 8;
let bodies = 0;
function leb() {
    let value = 0;
    let scale = 1;
    for (let index = 0; index < 5; index++) {
        assert.ok(at < bytes.length);
        const byte = bytes[at++];
        value += (byte & 127) * scale;
        if (!(byte & 128)) return value;
        scale *= 128;
    }
    assert.fail('invalid leb');
}
while (at < bytes.length) {
    const tag = bytes[at++];
    const end = leb() + at;
    assert.ok(end <= bytes.length);
    if (tag === 10) {
        assert.equal(leb(), 1);
        const body = leb();
        assert.equal(at + body, end);
        assert.equal(leb(), 2);
        assert.deepEqual([leb(), bytes[at++], leb(), bytes[at++]], [16, 127, 1, 126]);
        bodies++;
    }
    at = end;
}
assert.equal(at, bytes.length);
assert.equal(bodies, 1);
"#,
            guard,
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
}

#[test]
fn word_rol_six_profiles_caps_and_late_failures_preserve_both_owners() {
    let mut bank = Vec::new();
    for (bytes, _, _) in forms() {
        bank.extend(bytes);
    }
    bank.extend([0xeb, 0]);
    let engine = fixture(PC, &bank);
    for compiled in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(PC),
                byte_length: bank.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(PC)],
            CompileLimits::default(),
        )
        .unwrap(),
    ] {
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 25)
        );
        validate_profile(compiled.wasm_bytes(engine.memory().unwrap()).unwrap(), "");
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(PC, &bank);
            describe(&mut engine, PC, bank.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            validate_profile(module, if resident { "guard_resident" } else { "guard" });
            engine.map(0x3000, 1, 7).unwrap();
            upload(&mut engine, 0x3000, &[0x90, 0xeb, 0]);
            describe(&mut engine, 0x3000, 3, false);
            engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let generation = engine.generation();
            let old = engine.artifact_bytes().unwrap().to_vec();
            let old_resident = engine.resident_bytes(keep).unwrap().to_vec();
            for (pc, bytes, length, fault_pc, expected) in [
                (
                    PC,
                    [FORMS[0].to_vec(), vec![0x66, 0x66, 0xd1, 0xc0]].concat(),
                    7,
                    PC + 3,
                    InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                ),
                (
                    0x1ffc,
                    [FORMS[0].to_vec(), vec![0x66]].concat(),
                    5,
                    0x1fff,
                    InstructionError::Decode(DecodeError::MemoryFault {
                        pc: GuestAddress(0x1fff),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped,
                        },
                        length: 2,
                    }),
                ),
            ] {
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, length, entries);
                let arena = engine.arena().to_vec();
                let error = bound(&mut engine, resident, entries).unwrap_err();
                let observed = match error {
                    HostError::Compile(CompileError::Instruction { pc, cause })
                    | HostError::Resident(RegistryError::Compile(CompileError::Instruction {
                        pc,
                        cause,
                    })) => (pc, cause),
                    other => panic!("unexpected failure: {other:?}"),
                };
                assert_eq!(observed, (GuestAddress(fault_pc), expected));
                assert_eq!(engine.arena(), arena);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.artifact_bytes().unwrap(), old);
                assert_eq!(engine.resident_bytes(keep).unwrap(), old_resident);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            for nops in [62, 63] {
                if nops == 63 {
                    describe(&mut engine, 0x3000, 3, false);
                    engine.compile(1).unwrap();
                }
                let bytes = [FORMS[0].to_vec(), vec![0x90; nops], vec![0xeb, 0]].concat();
                upload(&mut engine, PC, &bytes);
                describe(&mut engine, PC, bytes.len(), entries);
                let arena = engine.arena().to_vec();
                let generation = engine.generation();
                let old = engine.artifact_bytes().unwrap().to_vec();
                let outcome = bound(&mut engine, resident, entries);
                if nops == 62 {
                    assert!(outcome.is_ok());
                } else {
                    assert_eq!(
                        outcome,
                        Err(if resident {
                            HostError::Resident(RegistryError::Compile(
                                CompileError::InstructionLimit,
                            ))
                        } else {
                            HostError::Compile(CompileError::InstructionLimit)
                        })
                    );
                    assert_eq!(engine.arena(), arena);
                    assert_eq!(engine.generation(), generation);
                    assert_eq!(engine.artifact_bytes().unwrap(), old);
                    assert_eq!(engine.resident_bytes(keep).unwrap(), old_resident);
                    engine.guard_resident(KEY, keep).unwrap();
                }
            }
        }
    }
}
