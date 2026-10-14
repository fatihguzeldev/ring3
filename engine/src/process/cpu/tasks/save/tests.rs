use super::*;
use crate::{
    abi::{arena::TRANSFER_OFFSET, x86::encode_state_current},
    cpu::x86::{
        SegmentDescriptorRecord, SegmentDescriptorTable, SegmentRegister, State32,
        tasks::{TaskFormat, TaskRegister},
    },
    memory::{Access, FaultReason, GuestAddress},
};

const KEY: u64 = 0x5453_5353_4156_4531;
const PAGES: [u32; 6] = [0, 0x1000, 0x2000, 0x5000, 0x6000, 0xffff_f000];

fn save() -> TaskSave {
    TaskSave {
        next_eip: 0x1234_5678,
        flags: 0x254fd7,
        general: [
            0x1122_3344,
            0x5566_7788,
            0x99aa_bbcc,
            0xddee_ff00,
            0x1357_9bdf,
            0x2468_ace0,
            0xdead_beef,
            0xcafe_babe,
        ],
        selectors: [0x23, 0x1b, 0x2b, 0x33, 0x3b, 0x43],
    }
}

fn descriptor(base: u32, limit: u32, format: TaskFormat) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | (limit & 0xffff),
        (base & 0xff00_0000)
            | ((base >> 16) & 0xff)
            | (limit & 0xf0000)
            | match format {
                TaskFormat::Word => 0x8300,
                TaskFormat::Dword => 0x8b00,
            },
    )
}

fn instance(
    base: u32,
    limit: u32,
    format: TaskFormat,
    resident: bool,
    entries: bool,
) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
        let bytes: Vec<u8> = (0..4096)
            .map(|index| index as u8 ^ (page >> 12) as u8 ^ 0xa5)
            .collect();
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(page), &bytes)
            .unwrap();
    }
    let current = TaskRegister::from_cached(0x43, descriptor(base, limit, format)).unwrap();
    engine.configure_task_return_sources(current, 0x4f).unwrap();
    engine
        .configure_segment_descriptor_raw(0x40, current.descriptor())
        .unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x2000, 2).unwrap();
    let words: &[u32] = if entries { &[0x2000] } else { &[0x2000, 2] };
    for (index, word) in words.iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let id = match (resident, entries) {
        (false, false) => {
            engine.compile(1).unwrap();
            None
        }
        (false, true) => {
            engine.compile_entries(1, 0).unwrap();
            None
        }
        (true, false) => Some(engine.compile_resident(1).unwrap().get()),
        (true, true) => Some(engine.compile_resident_entries(1, 0).unwrap().get()),
    };
    encode_state_current(
        &State32 {
            registers: save().general,
            eip: 0x2000,
            eflags: save().flags,
        },
        &mut engine.arena_mut().unwrap()[..56],
    )
    .unwrap();
    (engine, id)
}

fn invoke(engine: &mut EngineInstance, id: Option<u64>) -> Result<TaskSaveResult, HostError> {
    match id {
        Some(id) => engine.save_outgoing_task_resident(KEY, id, &save()),
        None => engine.save_outgoing_task(&save()),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    arena: Vec<u8>,
    ram: [Result<Vec<u8>, MemoryError>; 6],
    descriptors: SegmentDescriptorTable,
    sources: Option<super::super::TaskReturnSources>,
    version: u64,
}
fn snapshot(engine: &EngineInstance) -> Snapshot {
    let memory = engine.memory().unwrap();
    Snapshot {
        arena: engine.arena().to_vec(),
        ram: PAGES.map(|page| {
            let mut bytes = vec![0; 4096];
            memory.read(GuestAddress(page), &mut bytes).map(|()| bytes)
        }),
        descriptors: engine.segment_descriptors.clone(),
        sources: engine.task_return_sources,
        version: memory.code_validation_version(),
    }
}

fn literal_words(format: TaskFormat) -> Vec<(u16, u16)> {
    match format {
        TaskFormat::Word => vec![
            (14, 0x5678),
            (16, 0x0fd7),
            (18, 0x3344),
            (20, 0x7788),
            (22, 0xbbcc),
            (24, 0xff00),
            (26, 0x9bdf),
            (28, 0xace0),
            (30, 0xbeef),
            (32, 0xbabe),
            (34, 0x23),
            (36, 0x1b),
            (38, 0x2b),
            (40, 0x33),
        ],
        TaskFormat::Dword => vec![
            (32, 0x5678),
            (34, 0x1234),
            (36, 0x0fd7),
            (38, 0x25),
            (40, 0x3344),
            (42, 0x1122),
            (44, 0x7788),
            (46, 0x5566),
            (48, 0xbbcc),
            (50, 0x99aa),
            (52, 0xff00),
            (54, 0xddee),
            (56, 0x9bdf),
            (58, 0x1357),
            (60, 0xace0),
            (62, 0x2468),
            (64, 0xbeef),
            (66, 0xdead),
            (68, 0xbabe),
            (70, 0xcafe),
            (72, 0x23),
            (76, 0x1b),
            (80, 0x2b),
            (84, 0x33),
            (88, 0x3b),
            (92, 0x43),
        ],
    }
}

fn expected_save(
    before: &Snapshot,
    base: u32,
    format: TaskFormat,
) -> [Result<Vec<u8>, MemoryError>; 6] {
    let mut expected = before.ram.clone();
    for (offset, value) in literal_words(format) {
        for (byte, value) in value.to_le_bytes().into_iter().enumerate() {
            let address = base
                .wrapping_add(u32::from(offset))
                .wrapping_add(byte as u32);
            let page = PAGES
                .iter()
                .position(|page| *page == address & !4095)
                .unwrap();
            expected[page].as_mut().unwrap()[(address & 4095) as usize] = value;
        }
    }
    expected
}

#[test]
fn uniform_word_transaction_preserves_all_other_ram_cpu_and_busy_bytes_with_one_version() {
    for resident in [false, true] {
        for entries in [false, true] {
            for format in [TaskFormat::Word, TaskFormat::Dword] {
                let (mut engine, id) =
                    instance(0x5000, format.minimum_limit(), format, resident, entries);
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::Saved {
                        currentness: Ok(StoreCompletion::Complete)
                    })
                );
                let after = snapshot(&engine);
                assert_eq!(after.ram, expected_save(&before, 0x5000, format));
                assert_eq!(after.version, before.version + 1);
                assert_eq!(after.arena, before.arena);
                assert_eq!(after.descriptors, before.descriptors);
                assert_eq!(after.sources, before.sources);
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::Saved {
                        currentness: Ok(StoreCompletion::Complete)
                    })
                );
                let after = snapshot(&engine);
                assert_eq!(
                    after.ram, before.ram,
                    "identity save still publishes one version"
                );
                assert_eq!(after.version, before.version + 1);
                assert_eq!(after.arena, before.arena);
                assert_eq!(after.descriptors, before.descriptors);
                assert_eq!(after.sources, before.sources);
            }
        }
    }
}

#[test]
fn wrapped_outgoing_words_preserve_static_and_tail_bytes_on_both_sides_of_linear_zero() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (format, base) in [
                (TaskFormat::Word, 0xffff_fff1),
                (TaskFormat::Dword, 0xffff_ffdf),
            ] {
                let (mut engine, id) =
                    instance(base, format.minimum_limit(), format, resident, entries);
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::Saved {
                        currentness: Ok(StoreCompletion::Complete)
                    })
                );
                let after = snapshot(&engine);
                assert_eq!(after.ram, expected_save(&before, base, format));
                assert_eq!(after.version, before.version + 1);
                assert_eq!(after.arena, before.arena);
                assert_eq!(after.descriptors, before.descriptors);
                assert_eq!(after.sources, before.sources);
            }
        }
    }
}

#[test]
fn final_word_preflight_and_version_failure_never_publish_earlier_outgoing_fields() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (format, base) in [(TaskFormat::Word, 0x5fd7), (TaskFormat::Dword, 0x5fa3)] {
                for exhausted in [false, true] {
                    let (mut engine, id) =
                        instance(base, format.minimum_limit(), format, resident, entries);
                    engine.protect(0x6000, 1, 1).unwrap();
                    if exhausted {
                        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
                    }
                    let before = snapshot(&engine);
                    assert_eq!(
                        invoke(&mut engine, id),
                        Ok(TaskSaveResult::NeedsPagingAuthority {
                            fault: MemoryFault {
                                address: GuestAddress(0x6000),
                                access: Access::Write,
                                reason: FaultReason::Permission
                            },
                            length: 2
                        })
                    );
                    assert_eq!(snapshot(&engine), before);
                }
                let (mut engine, id) =
                    instance(base, format.minimum_limit(), format, resident, entries);
                engine.memory.as_mut().unwrap().exhaust_versions_for_test();
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::Infrastructure(
                        MemoryError::VersionExhausted
                    ))
                );
                assert_eq!(snapshot(&engine), before);
            }
        }
    }
}

#[test]
fn cached_limit_and_missing_authority_precede_all_outgoing_memory_accesses() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (format, limit) in [(TaskFormat::Word, 40), (TaskFormat::Dword, 92)] {
                let (mut engine, id) = instance(0xa000, limit, format, resident, entries);
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::InvalidTss(0x40))
                );
                assert_eq!(snapshot(&engine), before);
                engine.task_return_sources = None;
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::MissingAuthority)
                );
                assert_eq!(snapshot(&engine), before);
            }
        }
    }
}

#[test]
fn material_guards_are_prewrite_and_physical_smc_is_reported_after_the_save() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = instance(0x5000, 0x67, TaskFormat::Dword, resident, entries);
            if let Some(id) = id {
                let before = snapshot(&engine);
                assert_eq!(
                    engine.save_outgoing_task_resident(KEY ^ 1, id, &save()),
                    Err(HostError::InvalidArtifact)
                );
                assert_eq!(snapshot(&engine), before);
            }
            engine.set_segment_base(SegmentRegister::Cs, 0x100).unwrap();
            let before = snapshot(&engine);
            assert_eq!(invoke(&mut engine, id), Err(HostError::InvalidArtifact));
            assert_eq!(snapshot(&engine), before);
            for (format, base) in [(TaskFormat::Word, 0x1ff2), (TaskFormat::Dword, 0x1fe0)] {
                let (mut engine, id) =
                    instance(base, format.minimum_limit(), format, resident, entries);
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&mut engine, id),
                    Ok(TaskSaveResult::Saved {
                        currentness: Ok(StoreCompletion::CodeInvalidated)
                    })
                );
                let after = snapshot(&engine);
                assert_eq!(after.ram, expected_save(&before, base, format));
                assert_eq!(after.version, before.version + 1);
                assert_eq!(after.arena, before.arena);
                assert_eq!(after.descriptors, before.descriptors);
                assert_eq!(after.sources, before.sources);
            }
        }
    }
}
