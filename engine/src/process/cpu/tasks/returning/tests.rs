use super::*;
use crate::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, StateMetadata,
            encode_current_segment_context, encode_state_current,
        },
    },
    cpu::x86::tasks::TaskRegister,
    cpu::x86::{
        SegmentCache, SegmentDescriptorRecord, SegmentDescriptorTable, SegmentRegister, State32,
    },
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
};

const KEY: u64 = 0x5441_534b_5254_4e31;
const PAGES: [u32; 7] = [0, 0x1000, 0x5000, 0x6000, 0x8000, 0x9000, 0xffff_f000];

fn record(base: u32, limit: u32, access: u8) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | (limit & 0xffff),
        (base & 0xff00_0000) | ((base >> 16) & 0xff) | (u32::from(access) << 8) | (limit & 0xf0000),
    )
}

fn cached(base: u32, limit: u32) -> TaskRegister {
    TaskRegister::from_cached(0x43, record(base, limit, 0x8b)).unwrap()
}

fn backlink(engine: &mut EngineInstance, base: u32, selector: u16) {
    for (index, byte) in selector.to_le_bytes().into_iter().enumerate() {
        engine
            .write8(base.wrapping_add(index as u32), u32::from(byte))
            .unwrap();
    }
}

fn engine(
    current: Option<TaskRegister>,
    limit: u16,
    resident: bool,
    entries: bool,
) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(7, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    if let Some(current) = current {
        engine
            .configure_task_return_sources(current, limit)
            .unwrap();
    }
    engine
        .configure_segment_descriptor_raw(0x48, record(0x6000, 0x67, 0xeb))
        .unwrap();
    backlink(&mut engine, 0x5000, 0x4b);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    let words: &[u32] = if entries { &[0x1000] } else { &[0x1000, 2] };
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
    let mut context = engine.segment_context().unwrap();
    let ss = SegmentRegister::Ss.index();
    context.caches[ss] = SegmentCache::from_bits(context.caches[ss].bits() & !0x010f_ffff).unwrap();
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
    encode_state_current(
        &State32 {
            registers: [
                0x1234_5678,
                0x89ab_cdef,
                0x7654_3210,
                0xabcdef01,
                0xdead_000e,
                0x2345_6789,
                0x3456_789a,
                0x4567_89ab,
            ],
            eip: 0x1000,
            eflags: 0x254fd7,
        },
        &mut arena[..56],
    )
    .unwrap();
    StateMetadata {
        ss_debug_shadow: true,
    }
    .write((&mut arena[..56]).try_into().unwrap());
    (engine, id)
}

fn invoke(engine: &EngineInstance, id: Option<u64>) -> Result<TaskReturnAdmission, HostError> {
    match id {
        Some(id) => engine.prepare_task_return_resident(KEY, id),
        None => engine.prepare_task_return(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    arena: Vec<u8>,
    ram: [Result<Vec<u8>, MemoryError>; 7],
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

fn admitted(engine: &EngineInstance, id: Option<u64>, target: u16, base: u32, word: bool) {
    let before = snapshot(engine);
    let Ok(TaskReturnAdmission::Admitted(prepared)) = invoke(engine, id) else {
        panic!("expected prepared admission: {:?}", invoke(engine, id));
    };
    assert_eq!(
        (prepared.current().selector(), prepared.current().base()),
        (0x43, engine.task_return_sources.unwrap().current.base())
    );
    assert_eq!(
        (
            prepared.target().selector(),
            prepared.target().base(),
            prepared.target().format()
        ),
        (
            target,
            base,
            if word {
                crate::cpu::x86::tasks::TaskFormat::Word
            } else {
                crate::cpu::x86::tasks::TaskFormat::Dword
            }
        )
    );
    assert_eq!(snapshot(engine), before);
}

#[test]
fn live_backlinks_and_live_target_records_do_not_reload_cached_tr_or_touch_iret_stack() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = engine(Some(cached(0x5000, 0x67)), 0x57, resident, entries);
            engine
                .configure_segment_descriptor_raw(0x40, record(0x9000, 0x67, 0x8b))
                .unwrap();
            backlink(&mut engine, 0x9000, 0);
            admitted(&engine, id, 0x4b, 0x6000, false);
            backlink(&mut engine, 0x5000, 0x53);
            let before = snapshot(&engine);
            assert_eq!(
                invoke(&engine, id),
                Ok(TaskReturnAdmission::MissingAuthority)
            );
            assert_eq!(snapshot(&engine), before);
            engine
                .configure_segment_descriptor_raw(0x50, record(0, 0, 0))
                .unwrap();
            let before = snapshot(&engine);
            assert_eq!(
                invoke(&engine, id),
                Ok(TaskReturnAdmission::InvalidTss(0x50))
            );
            assert_eq!(snapshot(&engine), before);
            engine
                .configure_segment_descriptor_raw(0x50, record(0x9000, 0x2c, 0x83))
                .unwrap();
            admitted(&engine, id, 0x53, 0x9000, true);
        }
    }
}

#[test]
fn selected_target_faults_preserve_exact_selector_and_p6_presence_busy_limit_order() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = engine(Some(cached(0x5000, 0x67)), 0x4f, resident, entries);
            for (selector, descriptor, expected) in [
                (
                    0,
                    record(0x6000, 0x67, 0x8b),
                    TaskReturnAdmission::InvalidTss(0),
                ),
                (
                    0x4f,
                    record(0x6000, 0x67, 0x8b),
                    TaskReturnAdmission::InvalidTss(0x4c),
                ),
                (
                    0x53,
                    record(0x6000, 0x67, 0x8b),
                    TaskReturnAdmission::InvalidTss(0x50),
                ),
                (
                    0x4b,
                    record(0x6000, 0x67, 0x09),
                    TaskReturnAdmission::NotPresent(0x48),
                ),
                (
                    0x4b,
                    record(0x6000, 0x67, 0x89),
                    TaskReturnAdmission::InvalidTss(0x48),
                ),
                (
                    0x4b,
                    record(0x6000, 0x66, 0x0b),
                    TaskReturnAdmission::NotPresent(0x48),
                ),
                (
                    0x4b,
                    record(0x6000, 0x66, 0x8b),
                    TaskReturnAdmission::InvalidTss(0x48),
                ),
            ] {
                backlink(&mut engine, 0x5000, selector);
                engine
                    .configure_segment_descriptor_raw(0x48, descriptor)
                    .unwrap();
                let before = snapshot(&engine);
                assert_eq!(invoke(&engine, id), Ok(expected));
                assert_eq!(snapshot(&engine), before);
            }
        }
    }
}

#[test]
fn missing_configuration_and_current_limit_do_not_fabricate_backlink_or_page_faults() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (current, expected) in [
                (None, TaskReturnAdmission::MissingAuthority),
                (
                    Some(cached(0xa000, 0)),
                    TaskReturnAdmission::InvalidTss(0x40),
                ),
                (
                    Some(cached(0xa000, 1)),
                    TaskReturnAdmission::NeedsPagingAuthority {
                        fault: MemoryFault {
                            address: GuestAddress(0xa000),
                            access: Access::Read,
                            reason: FaultReason::Unmapped,
                        },
                        length: 2,
                    },
                ),
            ] {
                let (engine, id) = engine(current, 0x4f, resident, entries);
                let before = snapshot(&engine);
                assert_eq!(invoke(&engine, id), Ok(expected));
                assert_eq!(snapshot(&engine), before);
            }
        }
    }
}

#[test]
fn exactly_two_backlink_bytes_wrap_and_low_page_failures_remain_paging_authority_boundaries() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = engine(Some(cached(u32::MAX, 1)), 0x4f, resident, entries);
            backlink(&mut engine, u32::MAX, 0x4b);
            engine.write8(1, 0xff).unwrap();
            admitted(&engine, id, 0x4b, 0x6000, false);
            for (high_bits, low_bits, address) in [(7, 2, 0), (2, 2, u32::MAX)] {
                engine.protect(0xffff_f000, 1, high_bits).unwrap();
                engine.protect(0, 1, low_bits).unwrap();
                let before = snapshot(&engine);
                assert_eq!(
                    invoke(&engine, id),
                    Ok(TaskReturnAdmission::NeedsPagingAuthority {
                        fault: MemoryFault {
                            address: GuestAddress(address),
                            access: Access::Read,
                            reason: FaultReason::Permission
                        },
                        length: 2,
                    })
                );
                assert_eq!(snapshot(&engine), before);
            }
            engine.protect(0xffff_f000, 1, 7).unwrap();
            engine.unmap(0, 1).unwrap();
            let before = snapshot(&engine);
            assert_eq!(
                invoke(&engine, id),
                Ok(TaskReturnAdmission::NeedsPagingAuthority {
                    fault: MemoryFault {
                        address: GuestAddress(0),
                        access: Access::Read,
                        reason: FaultReason::Unmapped
                    },
                    length: 2,
                })
            );
            assert_eq!(snapshot(&engine), before);
        }
    }
}

#[test]
fn material_key_and_physical_currentness_reject_before_task_source_access() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = engine(Some(cached(0xa000, 0)), 0x4f, resident, entries);
            if let Some(id) = id {
                let before = snapshot(&engine);
                assert_eq!(
                    engine.prepare_task_return_resident(KEY ^ 1, id),
                    Err(HostError::InvalidArtifact)
                );
                assert_eq!(snapshot(&engine), before);
            }
            engine.set_segment_base(SegmentRegister::Cs, 0x100).unwrap();
            let before = snapshot(&engine);
            assert_eq!(invoke(&engine, id), Err(HostError::InvalidArtifact));
            assert_eq!(snapshot(&engine), before);
            engine.set_segment_base(SegmentRegister::Cs, 0).unwrap();
            let before = snapshot(&engine);
            assert_eq!(
                invoke(&engine, id),
                Ok(TaskReturnAdmission::InvalidTss(0x40))
            );
            assert_eq!(snapshot(&engine), before);
            engine.write8(0x1000, 0xeb).unwrap();
            let before = snapshot(&engine);
            assert_eq!(
                invoke(&engine, id),
                Err(if resident {
                    HostError::Resident(crate::cpu::dbt::RegistryError::CodeInvalidated)
                } else {
                    HostError::CodeInvalidated
                })
            );
            assert_eq!(snapshot(&engine), before);
        }
    }
}
