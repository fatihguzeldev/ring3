use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        x86::encode_state_v3,
    },
    cpu::x86::{SegmentDescriptorRecord, State32, tasks::TaskRegister},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError, StoreCompletion, TaskReturnAdmission},
};

const KEY: u64 = 0x5441_534b_4346;

fn descriptor(base: u32, limit: u32, access: u8) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | (limit & 0xffff),
        (base & 0xff00_0000) | ((base >> 16) & 255) | (u32::from(access) << 8) | (limit & 0xf0000),
    )
}

#[derive(Clone, Copy)]
struct Sources {
    configured: bool,
    base: u32,
    limit: u32,
    gdt: u16,
    backlink: u16,
    low: bool,
}
impl Default for Sources {
    fn default() -> Self {
        Self {
            configured: true,
            base: 0x5000,
            limit: 0x67,
            gdt: 0x4f,
            backlink: 0x4b,
            low: true,
        }
    }
}

fn engine(
    sources: Sources,
    target: Option<SegmentDescriptorRecord>,
    resident: bool,
    entries: bool,
    bytes: &[u8],
) -> (EngineInstance, Option<u64>, Vec<u32>) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    let mut pages = vec![0x1000, 0x5000, 0xffff_f000];
    if sources.low {
        pages.push(0);
    }
    for &page in &pages {
        engine.map(page, 1, 7).unwrap();
    }
    if sources.configured {
        let cached =
            TaskRegister::from_cached(0x43, descriptor(sources.base, sources.limit, 0x8b)).unwrap();
        engine
            .configure_task_return_sources(cached, sources.gdt)
            .unwrap();
    }
    // the live current descriptor is deliberately invalid; it must not reload hidden TR.
    engine
        .configure_segment_descriptor_raw(0x40, SegmentDescriptorRecord::from_words(0, 0))
        .unwrap();
    if let Some(target) = target {
        engine
            .configure_segment_descriptor_raw(0x48, target)
            .unwrap();
    }
    for (index, byte) in sources.backlink.to_le_bytes().into_iter().enumerate() {
        let address = sources.base.wrapping_add(index as u32);
        if pages.contains(&(address & !0xfff)) {
            engine.write8(address, u32::from(byte)).unwrap();
        }
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    let words = if entries {
        vec![0x1000]
    } else {
        vec![0x1000, bytes.len() as u32]
    };
    for (index, word) in words.into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
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
    encode_state_v3(
        &State32 {
            registers: [
                0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0xdead000e, 0x56789abc, 0x6789abcd,
                0x789abcde,
            ],
            eip: 0x1000,
            eflags: 0x254fd7,
        },
        &mut engine.arena_mut().unwrap()[..56],
    )
    .unwrap();
    engine.arena_mut().unwrap()[12..16].copy_from_slice(&1_u32.to_le_bytes());
    (engine, id, pages)
}

fn ram(engine: &EngineInstance, pages: &[u32]) -> Vec<Vec<u8>> {
    pages
        .iter()
        .map(|&page| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(page), &mut bytes)
                .unwrap();
            bytes
        })
        .collect()
}

fn packet(fields: [u32; 6]) -> Vec<u8> {
    let mut bytes = b"R3MH\x0d\0\x01\0\x28\0\0\0\0\0\0\0".to_vec();
    for value in fields {
        bytes.extend(value.to_le_bytes());
    }
    bytes
}

fn prepare(engine: &EngineInstance, id: Option<u64>) -> Result<TaskReturnAdmission, HostError> {
    match id {
        Some(id) => engine.prepare_task_return_resident(KEY, id),
        None => engine.prepare_task_return(),
    }
}
fn helper(engine: &mut EngineInstance, id: Option<u64>) -> Result<StoreCompletion, HostError> {
    match id {
        Some(id) => engine.task_return_resident(KEY, id),
        None => engine.task_return(),
    }
}

fn check(engine: &mut EngineInstance, id: Option<u64>, pages: &[u32], fields: [u32; 6]) {
    let before = engine.arena().to_vec();
    let expected_ram = ram(engine, pages);
    let snapshots: Vec<_> = pages
        .iter()
        .map(|&page| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(page), 4096)
                .unwrap()
        })
        .collect();
    let result = prepare(engine, id).unwrap();
    match fields[0] {
        3 => assert_eq!(result, TaskReturnAdmission::InvalidTss(fields[2])),
        6 => assert_eq!(result, TaskReturnAdmission::NotPresent(fields[2])),
        8 if fields[2] == 1 => assert_eq!(result, TaskReturnAdmission::MissingAuthority),
        8 => {
            let TaskReturnAdmission::Admitted(prepared) = result else {
                panic!("{result:?}")
            };
            assert_eq!(
                (
                    prepared.current().selector(),
                    prepared.target().selector(),
                    prepared.target().base()
                ),
                (0x43, fields[3] as u16, 0x9000)
            );
        }
        1 => assert_eq!(
            result,
            TaskReturnAdmission::NeedsPagingAuthority {
                fault: MemoryFault {
                    address: GuestAddress(fields[3]),
                    access: Access::Read,
                    reason: FaultReason::Unmapped
                },
                length: 2
            }
        ),
        _ => panic!("unexpected literal outcome"),
    }
    assert_eq!(engine.arena(), before, "pure admission publishes nothing");
    assert_eq!(ram(engine, pages), expected_ram);
    assert_eq!(helper(engine, id), Ok(StoreCompletion::Complete));
    let mut expected = before;
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(fields));
    assert_eq!(
        engine.arena(),
        expected,
        "native helper changes only its exact13packet"
    );
    assert_eq!(
        ram(engine, pages),
        expected_ram,
        "no outgoing save, busy-bit update or stack write"
    );
    for snapshot in &snapshots {
        assert!(engine.memory().unwrap().is_code_current(snapshot));
    }
}

#[test]
fn target_authority_zero_presence_busy_and_two_tss_formats_have_literal_packets() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (target, fields) in [
                (None, [8, 0, 1, 0, 0, 0]),
                (
                    Some(SegmentDescriptorRecord::from_words(0, 0)),
                    [3, 0, 0x48, 0, 0, 0],
                ),
                (Some(descriptor(0x9000, 0, 0x09)), [6, 0, 0x48, 0, 0, 0]),
                (Some(descriptor(0x9000, 0x67, 0x89)), [3, 0, 0x48, 0, 0, 0]),
                (Some(descriptor(0x9000, 0x66, 0x8b)), [3, 0, 0x48, 0, 0, 0]),
                (Some(descriptor(0x9000, 0x67, 0x8b)), [8, 0, 2, 0x4b, 0, 0]),
                (Some(descriptor(0x9000, 0x2c, 0xe3)), [8, 0, 2, 0x4b, 0, 0]),
            ] {
                let (mut engine, id, pages) =
                    engine(Sources::default(), target, resident, entries, &[0xcf]);
                check(&mut engine, id, &pages, fields);
            }
        }
    }
}

#[test]
fn cached_limit_null_ti_and_wrapping_backlink_keep_exact_source_priority() {
    for resident in [false, true] {
        for entries in [false, true] {
            for (sources, fields) in [
                (
                    Sources {
                        configured: false,
                        ..Default::default()
                    },
                    [8, 0, 1, 0, 0, 0],
                ),
                (
                    Sources {
                        base: 0xdead0000,
                        limit: 0,
                        ..Default::default()
                    },
                    [3, 0, 0x40, 0, 0, 0],
                ),
                (
                    Sources {
                        backlink: 0,
                        ..Default::default()
                    },
                    [3, 0, 0, 0, 0, 0],
                ),
                (
                    Sources {
                        backlink: 0x4f,
                        ..Default::default()
                    },
                    [3, 0, 0x4c, 0, 0, 0],
                ),
                (
                    Sources {
                        gdt: 0x3f,
                        backlink: 0,
                        ..Default::default()
                    },
                    [3, 0, 0, 0, 0, 0],
                ),
                (
                    Sources {
                        base: u32::MAX,
                        ..Default::default()
                    },
                    [8, 0, 2, 0x4b, 0, 0],
                ),
                (
                    Sources {
                        base: u32::MAX,
                        low: false,
                        ..Default::default()
                    },
                    [1, 0, 1, 0, 1, 2],
                ),
                (
                    Sources {
                        base: 0xdead0000,
                        ..Default::default()
                    },
                    [1, 0, 1, 0xdead0000, 1, 2],
                ),
            ] {
                let (mut engine, id, pages) = engine(
                    sources,
                    Some(descriptor(0x9000, 0x67, 0x8b)),
                    resident,
                    entries,
                    &[0x66, 0xcf],
                );
                check(&mut engine, id, &pages, fields);
            }
        }
    }
}

#[test]
fn live_backlink_is_reread_and_wrong_owner_precedes_helper_publication() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id, pages) = engine(
                Sources::default(),
                Some(descriptor(0x9000, 0x67, 0x8b)),
                resident,
                entries,
                &[0xcf],
            );
            check(&mut engine, id, &pages, [8, 0, 2, 0x4b, 0, 0]);
            engine.write8(0x5000, 0).unwrap();
            check(&mut engine, id, &pages, [3, 0, 0, 0, 0, 0]);
            if let Some(id) = id {
                let before = engine.arena().to_vec();
                let memory = ram(&engine, &pages);
                assert_eq!(
                    engine.task_return_resident(KEY ^ 1, id),
                    Err(HostError::InvalidArtifact)
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(ram(&engine, &pages), memory);
            }
        }
    }
}

#[test]
fn configuration_freezes_before_material_and_does_not_require_current_tr_in_live_gdt() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let current = TaskRegister::from_cached(0x43, descriptor(0xdead0000, 0, 0x8b)).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.configure_task_return_sources(current, 0), Ok(()));
    assert_eq!(
        engine.arena(),
        before,
        "configuration does not fetch cached TR or consult live GDT"
    );
    assert_eq!(
        engine.configure_task_return_sources(current, 0xffff),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(
        engine.configure_task_return_sources(current, 0),
        Err(HostError::Closed)
    );
}
