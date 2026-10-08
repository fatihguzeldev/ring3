use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admission(modrm: u8) {
    let bytes = [0xf6, modrm, 0xeb, 0];
    let mut engine = EngineInstance::new(1, 1).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine.compile(1).expect("memory byte multiply admission");
}

#[test]
fn memory_byte_mul_admits_public_api() {
    admission(0x23);
}

#[test]
fn memory_byte_imul_admits_public_api() {
    admission(0x2b);
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, MultiplyKind, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fixture(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 3, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&pc.to_le_bytes());
    request[4..].copy_from_slice(&if entries { 0 } else { length as u32 }.to_le_bytes());
}

fn bound(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn encoded(kind: MultiplyKind, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xf6];
    bytes.extend_from_slice(tail);
    bytes[1] |= if kind == MultiplyKind::Unsigned {
        0x20
    } else {
        0x28
    };
    bytes
}

fn forms() -> Vec<(Vec<u8>, EffectiveAddress)> {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esp};
    [
        (vec![0x03], Some(Ebx), None, 1, 0),
        (vec![0x00], Some(Eax), None, 1, 0),
        (vec![0x02], Some(Edx), None, 1, 0),
        (vec![0x04, 0x24], Some(Esp), None, 1, 0),
        (vec![0x44, 0x8f, 0x80], Some(Edi), Some(Ecx), 4, 0xffff_ff80),
        (vec![0x45, 0x80], Some(Ebp), None, 1, 0xffff_ff80),
        (vec![0x83, 0x10, 0, 0, 0], Some(Ebx), None, 1, 0x10),
        (vec![0x05, 0x10, 0x50, 0, 0], None, None, 1, 0x5010),
    ]
    .into_iter()
    .map(|(tail, base, index, scale, displacement)| {
        (
            tail,
            EffectiveAddress {
                base,
                index,
                scale,
                displacement,
            },
        )
    })
    .collect()
}

#[test]
fn memory_byte_ea_identities_need_read8_in_four_bound_profiles() {
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for kind in [MultiplyKind::Unsigned, MultiplyKind::Signed] {
        for (tail, address) in forms() {
            let form = encoded(kind, &tail);
            expected.push((CODE + bytes.len() as u32, form.len(), kind, address));
            bytes.extend(form);
        }
    }
    bytes.extend([0xeb, 0]);
    let engine = fixture(CODE, &bytes);
    for (pc, length, kind, address) in expected {
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ReadByteMultiplyAccumulator { kind, address }
        );
        assert_eq!(
            (decoded.length() as usize, decoded.next_pc()),
            (length, GuestAddress(pc + length as u32))
        );
    }
    let unsupported = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32
            }],
            CompileLimits::default()
        )
        .err(),
        Some(unsupported)
    );
    assert_eq!(
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(CODE)],
            CompileLimits::default()
        )
        .err(),
        Some(unsupported)
    );
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/r449-memory-byte-multiply")
        .join(format!(
            "native-modules-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let mut command = Command::new("node");
    command.arg("-e").arg("const f=require('node:fs'),a=require('node:assert/strict');for(let i=1;i<process.argv.length;i+=2){const m=new WebAssembly.Module(f.readFileSync(process.argv[i]));a.deepEqual(WebAssembly.Module.imports(m),[{module:'env',name:'memory',kind:'memory'},{module:'ring3',name:process.argv[i+1],kind:'function'},{module:'ring3',name:'read8',kind:'function'}]);a.deepEqual(WebAssembly.Module.exports(m),[{name:'run',kind:'function'}]);}");
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(CODE, &bytes);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            let module = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            let path = output.join(format!("{resident}-{entries}.wasm"));
            fs::write(&path, module).unwrap();
            command
                .arg(path)
                .arg(if resident { "guard_resident" } else { "guard" });
        }
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn strict_neighbors_and_fetch_boundaries_keep_exact_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for kind in [MultiplyKind::Unsigned, MultiplyKind::Signed] {
        for (prefix, wanted) in [
            (0x66, opcode),
            (0x67, opcode),
            (0xf2, opcode),
            (0xf3, opcode),
            (0x64, DecodeError::Unsupported(UnsupportedFeature::Segment)),
            (0xf0, DecodeError::InvalidEncoding),
        ] {
            let mut bytes = vec![prefix];
            bytes.extend(encoded(kind, &[0x03]));
            let engine = fixture(CODE, &bytes);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
                Some(wanted)
            );
        }
        for (tail, _) in forms()
            .into_iter()
            .filter(|(tail, _)| matches!(tail.as_slice(), [0x03] | [0x05, ..] | [0x44, ..]))
        {
            let bytes = encoded(kind, &tail);
            for pc in [
                0x2000 - bytes.len() as u32,
                u32::MAX - (bytes.len() as u32 - 1),
                0x1fff,
            ] {
                let engine = fixture(pc, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                assert_eq!(
                    (decoded.length() as usize, decoded.next_pc()),
                    (
                        bytes.len(),
                        GuestAddress(pc.wrapping_add(bytes.len() as u32))
                    )
                );
            }
            if bytes.len() > 2 {
                let partial = &bytes[..bytes.len() - 1];
                let pc = 0x2000 - partial.len() as u32;
                let engine = fixture(pc, partial);
                assert_eq!(
                    decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                    Some(DecodeError::MemoryFault {
                        pc: GuestAddress(pc),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped
                        },
                        length: bytes.len() as u32
                    })
                );
            }
        }
    }
    for bytes in [
        &[0x66, 0xf6, 0x33][..],
        &[0x66, 0xf6, 0x3b],
        &[0x66, 0xf7, 0x23],
        &[0x66, 0xf7, 0x2b],
    ] {
        let engine = fixture(CODE, bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(opcode)
        );
    }
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Unmapped),
        (0x1fff, 0x2000, FaultReason::Permission),
        (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
    ] {
        let mut engine = fixture(pc, &[0xf6]);
        if reason == FaultReason::Permission {
            engine.map(0x2000, 1, 3).unwrap();
        }
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(address),
                    access: Access::Execute,
                    reason
                },
                length: 2
            })
        );
    }
}

#[test]
fn data_currency_late_refusals_and_caps_preserve_all_publications() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fixture(CODE, &[0x90]);
            engine.protect(CODE, 1, 7).unwrap();
            engine.map(KEEP, 1, 7).unwrap();
            upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
            describe(&mut engine, KEEP, 3, false);
            let generation = engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let modules = (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
            );
            engine.map(0x5000, 1, 3).unwrap();
            for value in [0, 5, 0xff] {
                upload(&mut engine, 0x5010, &[value]);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            engine.protect(0x5000, 1, 2).unwrap();
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            let mut failures = Vec::new();
            for modrm in [0x23, 0x2b] {
                for tail in [vec![0x0f, 0x0b], vec![0x66, 0xf6, modrm]] {
                    let mut bytes = vec![0xf6, modrm];
                    bytes.extend(tail);
                    let length = bytes.len();
                    failures.push((
                        CODE,
                        bytes,
                        length,
                        CompileError::Instruction {
                            pc: GuestAddress(CODE + 2),
                            cause: InstructionError::Decode(DecodeError::Unsupported(
                                UnsupportedFeature::Opcode,
                            )),
                        },
                    ));
                }
            }
            failures.push((
                0x1fff,
                vec![0xf6],
                2,
                CompileError::Instruction {
                    pc: GuestAddress(0x1fff),
                    cause: InstructionError::Decode(DecodeError::MemoryFault {
                        pc: GuestAddress(0x1fff),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped,
                        },
                        length: 2,
                    }),
                },
            ));
            let mut over = [0xf6, 0x23].repeat(64);
            over.extend([0xeb, 0]);
            let length = over.len();
            failures.push((CODE, over, length, CompileError::InstructionLimit));
            for (pc, bytes, length, failure) in failures {
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, length, entries);
                let before = engine.arena().to_vec();
                let wanted = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                assert_eq!(bound(&mut engine, resident, entries), Err(wanted));
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.artifact_bytes().unwrap(), modules.0);
                assert_eq!(engine.resident_bytes(keep).unwrap(), modules.1);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            for modrm in [0x23, 0x2b] {
                let mut at_limit = [0xf6, modrm].repeat(63);
                at_limit.extend([0xeb, 0]);
                upload(&mut engine, CODE, &at_limit);
                describe(&mut engine, CODE, at_limit.len(), entries);
                bound(&mut engine, resident, entries).unwrap();
            }
        }
    }
}
