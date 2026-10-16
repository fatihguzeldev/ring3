use super::support_prefix_admission as prefix_admission;

use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admission(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 1).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("memory BT must admit via public API");
    engine
}

#[test]
fn memory_bit_test_register_index_admits_public_api() {
    admission(&[0x0f, 0xa3, 0x0b, 0xeb, 0]);
}

#[test]
fn memory_bit_test_immediate_index_admits_public_api() {
    admission(&[0x0f, 0xba, 0x23, 0xff, 0xeb, 0]);
}

use std::{fs::OpenOptions, io::Write};

#[test]
fn existing_read32_module_remains_valid_across_address_bridge() {
    let engine = admission(&[0x8b, 0x03, 0xeb, 0]);
    let bytes = engine.artifact_bytes().unwrap();
    crate::support_wasm_interface::assert_bound(
        bytes,
        false,
        &[("cpu_read32", 1)],
        &[4, 6, 1],
        true,
    );
    if let Some(path) = std::env::var_os("RING3_BT_READ32_OUTPUT") {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
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
            ir::{BitIndex, EffectiveAddress, Operation},
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
fn encoded(index: BitIndex, tail: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0x0f,
        if matches!(index, BitIndex::Immediate(_)) {
            0xba
        } else {
            0xa3
        },
    ];
    bytes.extend_from_slice(tail);
    match index {
        BitIndex::Register(source) => bytes[2] |= (source as u8) << 3,
        BitIndex::Immediate(raw) => {
            bytes[2] |= 0x20;
            bytes.push(raw);
        }
    }
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
                ..Default::default()
            },
        )
    })
    .collect()
}

#[test]
fn memory_bit_test_ea_index_identities_require_read32_in_four_bound_profiles() {
    let mut rows = Vec::new();
    for index in [
        BitIndex::Register(Register32::Ecx),
        BitIndex::Immediate(255),
    ] {
        for (tail, address) in forms() {
            rows.push((index, tail, address));
        }
    }
    for source in [
        Register32::Eax,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ] {
        rows.push((BitIndex::Register(source), vec![0x03], forms()[0].1));
    }
    for raw in [0, 31, 32] {
        rows.push((BitIndex::Immediate(raw), vec![0x03], forms()[0].1));
    }
    assert_eq!(rows.len(), 26);
    let mut bytes = Vec::new();
    let mut expected = Vec::new();
    for (index, tail, address) in rows {
        let form = encoded(index, &tail);
        expected.push((CODE + bytes.len() as u32, form.len(), address, index));
        bytes.extend(form);
    }
    bytes.extend([0xeb, 0]);
    let engine = fixture(CODE, &bytes);
    for (pc, length, address, index) in expected {
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ReadBitTest { address, index }
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
        .join("target/r450-bt")
        .join(format!(
            "native-modules-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
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
            crate::support_wasm_interface::assert_bound(
                module,
                resident,
                &[("cpu_read32", 1)],
                &[4, if resident { 7 } else { 6 }, 1],
                true,
            );
        }
    }
}

#[test]
fn strict_neighbors_and_fetch_boundaries_keep_exact_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for index in [
        BitIndex::Register(Register32::Ecx),
        BitIndex::Immediate(255),
    ] {
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
        ] {
            let mut bytes = if prefix == 0x66 {
                vec![0x66, 0x66]
            } else {
                vec![prefix]
            };
            bytes.extend(encoded(index, &[0x03]));
            let wanted = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode,
            };
            if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x67) {
                prefix_admission::admitted(&bytes);
                continue;
            }
            crate::support_fetch::assert_decode_contract(
                (decode_one(fixture(CODE, &bytes).memory().unwrap(), GuestAddress(CODE)))
                    .expect_err("instruction must fail to decode"),
                wanted,
            );
        }
        for (tail, _) in forms()
            .into_iter()
            .filter(|(tail, _)| matches!(tail.as_slice(), [0x03] | [0x05, ..] | [0x44, ..]))
        {
            let bytes = encoded(index, &tail);
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
        }
    }
    for bytes in [
        &[0x66, 0x66, 0x0f, 0xab, 0x0b][..],
        &[0x66, 0x66, 0x0f, 0xb3, 0x0b],
        &[0x66, 0x66, 0x0f, 0xbb, 0x0b],
        &[0x66, 0x66, 0x0f, 0xba, 0x2b, 255],
        &[0x66, 0x66, 0x0f, 0xba, 0x33, 255],
        &[0x66, 0x66, 0x0f, 0xba, 0x3b, 255],
    ] {
        crate::support_fetch::assert_decode_contract(
            (decode_one(fixture(CODE, bytes).memory().unwrap(), GuestAddress(CODE)))
                .expect_err("instruction must fail to decode"),
            opcode,
        );
    }
    for partial in [
        &[0x0f][..],
        &[0x0f, 0xa3],
        &[0x0f, 0xa3, 0x04],
        &[0x0f, 0xba, 0x23],
    ] {
        let pc = 0x2000 - partial.len() as u32;
        let engine = fixture(pc, partial);
        crate::support_fetch::assert_error(
            (decode_one(engine.memory().unwrap(), GuestAddress(pc)))
                .expect_err("code fetch must fail"),
            pc,
            0x2000,
            FaultReason::Unmapped,
            partial.len() as u32 + 1,
        );
    }
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Permission),
        (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
    ] {
        let mut engine = fixture(pc, &[0x0f]);
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
fn data_currency_late_refusals_and_caps_preserve_all_publications() {
    assert_eq!(
        CompileLimits::default(),
        CompileLimits {
            blocks: 8,
            instructions: 64,
            wasm_bytes: 65536
        }
    );
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
            for value in [0_u32, 0x8000_0000, u32::MAX] {
                upload(&mut engine, 0x5010, &value.to_le_bytes());
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            engine.protect(0x5000, 1, 2).unwrap();
            engine.unmap(0x5000, 1).unwrap();
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            let mut failures = Vec::new();
            for index in [
                BitIndex::Register(Register32::Ecx),
                BitIndex::Immediate(255),
            ] {
                let form = encoded(index, &[0x03]);
                for (tail, error) in [
                    (vec![0xf0, 0x90], DecodeError::InvalidEncoding),
                    (
                        {
                            let mut t = vec![0x66, 0x66];
                            t.extend_from_slice(&form);
                            t
                        },
                        DecodeError::Unsupported(UnsupportedFeature::Opcode),
                    ),
                ] {
                    let mut bytes = form.clone();
                    let late = CODE + bytes.len() as u32;
                    bytes.extend(tail);
                    let length = bytes.len();
                    failures.push((
                        CODE,
                        bytes,
                        length,
                        CompileError::Instruction {
                            pc: GuestAddress(late),
                            cause: InstructionError::Decode(error),
                        },
                    ));
                }
            }
            let late_prefix = encoded(BitIndex::Register(Register32::Ecx), &[0x03]);
            let start = 0x2000 - late_prefix.len() as u32 - 1;
            let mut partial = late_prefix;
            partial.push(0x0f);
            failures.push((
                start,
                partial,
                5,
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
            let mut over = [0x0f, 0xa3, 0x0b].repeat(64);
            over.extend([0xeb, 0]);
            let length = over.len();
            failures.push((CODE, over, length, CompileError::InstructionLimit));
            for (pc, bytes, length, failure) in failures {
                upload(&mut engine, pc, &bytes);
                describe(&mut engine, pc, length, entries);
                if entries
                    && crate::support_admission::admit_if_deferred(
                        &mut engine,
                        crate::support_admission::ColdEntryCase {
                            key: KEY,
                            keeper: keep,
                            resident,
                            pages: &[CODE, KEEP, 0x5000],
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
                assert_eq!(engine.artifact_bytes().unwrap(), modules.0);
                assert_eq!(engine.resident_bytes(keep).unwrap(), modules.1);
                engine.guard(KEY, generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
            }
            for index in [
                BitIndex::Register(Register32::Ecx),
                BitIndex::Immediate(255),
            ] {
                let mut at_limit = encoded(index, &[0x03]).repeat(63);
                at_limit.extend([0xeb, 0]);
                let mut oversized = fixture(CODE, &at_limit);
                crate::support_admission::assert_oversized_explicit(
                    &mut oversized,
                    CODE,
                    at_limit.len() as u32,
                    resident,
                );
                crate::support_memory_banks::assert_bounded_memory_bank(
                    &at_limit, CODE, KEY, resident, entries, 64,
                );
                let mut at_limit = encoded(index, &[0x03]).repeat(7);
                at_limit.extend(std::iter::repeat_n(0x90, 56));
                at_limit.extend([0xeb, 0]);
                upload(&mut engine, CODE, &at_limit);
                describe(&mut engine, CODE, at_limit.len(), entries);
                let before = engine.arena().to_vec();
                let id = bound(&mut engine, resident, entries).unwrap();
                assert_eq!(engine.arena(), before);
                engine.map(0x5000, 1, 3).unwrap();
                upload(&mut engine, 0x5010, &[1, 0, 0, 0]);
                if resident {
                    engine.guard_resident(KEY, id).unwrap();
                } else {
                    engine.guard(KEY, id as u32).unwrap();
                }
                upload(&mut engine, CODE, &at_limit[..1]);
                if resident {
                    assert!(engine.guard_resident(KEY, id).is_err());
                } else {
                    assert!(engine.guard(KEY, id as u32).is_err());
                }
                engine.unmap(0x5000, 1).unwrap();
            }
        }
    }
}
