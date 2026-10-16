use super::support_prefix_admission as prefix_admission;

use super::support_byte_prefix_admission as byte_prefix_admission;

use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admission(modrm: u8) {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&[0xf6, modrm, 0xeb, 0]);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register BYTE division admits through the public compiler");
}

#[test]
fn register_div_byte_ah_admits_public_api() {
    admission(0xf4);
}

#[test]
fn register_idiv_byte_ah_admits_public_api() {
    admission(0xfc);
}

use ring3_engine::{
    abi::arena::TRANSFER_SIZE,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, DivideKind, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const SOURCES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn fresh(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
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

fn instruction_error(pc: u32, decode: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(decode),
    }
}

#[test]
fn all_byte_sources_have_exact_ir_and_six_valid_module_profiles() {
    let mut bytes = Vec::new();
    for base in [0xf0, 0xf8] {
        for source in 0..8 {
            bytes.extend([0xf6, base | source]);
        }
    }
    bytes.extend([0xeb, 0]);
    let engine = fresh(CODE, &bytes);
    let memory = engine.memory().unwrap();
    for (group, kind) in [DivideKind::Unsigned, DivideKind::Signed]
        .into_iter()
        .enumerate()
    {
        for (index, source) in SOURCES.into_iter().enumerate() {
            let pc = CODE + (group * 16 + index * 2) as u32;
            let instruction = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                instruction.operation(),
                &Operation::ByteDivideAccumulator { kind, source }
            );
            assert_eq!(
                (instruction.length(), instruction.next_pc()),
                (2, GuestAddress(pc + 2))
            );
        }
    }
    let mut modules = Vec::new();
    for region in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 34,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(region.metadata().instructions, 17);
        modules.push(("", region.wasm_bytes(memory).unwrap().to_vec()));
    }
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fresh(CODE, &bytes);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let id = bound(&mut engine, resident, entries).unwrap();
            assert_eq!(engine.arena(), before);
            let bytes = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            modules.push((
                if resident { "guard_resident" } else { "guard" },
                bytes.to_vec(),
            ));
        }
    }
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/r447-byte-division")
        .join(format!(
            "native-modules-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    for (index, (guard, bytes)) in modules.iter().enumerate() {
        let path = output.join(format!("{index}.wasm"));
        fs::write(&path, bytes).unwrap();
        if guard.is_empty() {
            crate::support_wasm_interface::assert_interface(bytes, &[], &[4], false);
        } else {
            let resident = *guard == "guard_resident";
            crate::support_wasm_interface::assert_bound(
                bytes,
                resident,
                &[],
                &[4, if resident { 7 } else { 6 }],
                false,
            );
        }
    }
    assert_eq!(modules.len(), 6);
}

#[test]
fn strict_memory_word_prefix_and_two_byte_fetch_boundaries_remain_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for base in [0xf0, 0xf8] {
        for (prefix, expected) in [
            (0x66, opcode),
            (0x67, opcode),
            (0xf2, opcode),
            (0xf3, opcode),
            (0x64, DecodeError::Unsupported(UnsupportedFeature::Segment)),
            (0xf0, DecodeError::InvalidEncoding),
        ] {
            let (bytes, expected) =
                byte_prefix_admission::closed_fixture(&[prefix, 0xf6, base | 4], expected);
            let engine = fresh(CODE, &bytes);
            crate::support_fetch::assert_decode_contract(
                (decode_one(engine.memory().unwrap(), GuestAddress(CODE)))
                    .expect_err("instruction must fail to decode"),
                expected,
            );
        }
        for bytes in [
            vec![0x66, 0xf6, base & 0x3f | 3],
            vec![0x66, 0x66, 0xf7, base],
        ] {
            let engine = fresh(CODE, &bytes);
            if !prefix_admission::admit_if_new_prefix(&bytes) {
                crate::support_fetch::assert_decode_contract(
                    (decode_one(engine.memory().unwrap(), GuestAddress(CODE)))
                        .expect_err("instruction must fail to decode"),
                    opcode,
                );
            }
        }
        for pc in [0x1ffe, 0x1fff, 0xffff_fffe] {
            let engine = fresh(pc, &[0xf6, base | 4]);
            let instruction = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(
                (instruction.length(), instruction.next_pc()),
                (2, GuestAddress(pc.wrapping_add(2)))
            );
            assert_eq!(
                compile_region(
                    engine.memory().unwrap(),
                    &[BlockSpec {
                        entry: GuestAddress(pc),
                        byte_length: 2
                    }],
                    CompileLimits::default()
                )
                .unwrap()
                .metadata()
                .instructions,
                1
            );
        }
    }
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Unmapped),
        (0x1fff, 0x2000, FaultReason::Permission),
        (0xffff_ffff, 0xffff_ffff, FaultReason::AddressOverflow),
    ] {
        let mut engine = fresh(pc, &[0xf6]);
        if reason == FaultReason::Permission {
            engine.map(0x2000, 1, 3).unwrap();
        }
        crate::support_fetch::assert_error(
            (decode_one(engine.memory().unwrap(), GuestAddress(pc)))
                .expect_err("code fetch must fail"),
            pc,
            address,
            reason,
            2,
        );
    }
}

#[test]
fn late_refusals_and_instruction_caps_preserve_both_published_owners() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = fresh(CODE, &[0x90]);
            engine.protect(CODE, 1, 7).unwrap();
            engine.map(KEEP, 1, 7).unwrap();
            upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
            describe(&mut engine, KEEP, 3, false);
            let generation = engine.compile(1).unwrap();
            let keep = engine.compile_resident(1).unwrap().get();
            let artifacts = (
                engine.artifact_bytes().unwrap().to_vec(),
                engine.resident_bytes(keep).unwrap().to_vec(),
            );
            let mut cases = Vec::new();
            for modrm in [0xf4, 0xfc] {
                for tail in [vec![0xf0, 0x90], vec![0x66, 0xf6, modrm]] {
                    let cause = if tail == [0xf0, 0x90] {
                        DecodeError::InvalidEncoding
                    } else {
                        DecodeError::Unsupported(UnsupportedFeature::Opcode)
                    };
                    let (tail, cause) = byte_prefix_admission::closed_fixture(&tail, cause);
                    let mut bytes = vec![0xf6, modrm];
                    bytes.extend(tail);
                    let length = bytes.len();
                    cases.push((CODE, bytes, length, instruction_error(CODE + 2, cause)));
                }
            }
            cases.push((
                0x1fff,
                vec![0xf6],
                2,
                instruction_error(
                    0x1fff,
                    DecodeError::MemoryFault {
                        pc: GuestAddress(0x1fff),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped,
                        },
                        length: 2,
                    },
                ),
            ));
            let mut over = [0xf6, 0xf4].repeat(64);
            over.extend([0xeb, 0]);
            let length = over.len();
            cases.push((CODE, over, length, CompileError::InstructionLimit));
            for (pc, bytes, length, failure) in cases {
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, length, entries);
                if entries
                    && crate::support_admission::admit_if_deferred(
                        &mut engine,
                        crate::support_admission::ColdEntryCase {
                            key: KEY,
                            keeper: keep,
                            resident,
                            pages: &[CODE, KEEP],
                            expected: failure,
                        },
                    )
                {
                    continue;
                }
                let before = engine.arena().to_vec();
                let wanted = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                crate::support_fetch::assert_host_contract(
                    (bound(&mut engine, resident, entries))
                        .expect_err("compilation must refuse this fixture"),
                    wanted,
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.artifact_bytes().unwrap(), artifacts.0);
                assert_eq!(engine.resident_bytes(keep).unwrap(), artifacts.1);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
            }
        }
    }
}
