use super::*;
use crate::abi::arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET};

const WORD_GPR: [u32; 8] = [
    0x3344, 0x7788, 0xbbcc, 0xff00, 0x9bdf, 0xace0, 0xbeef, 0xbabe,
];

fn saved_word_ram(mut before: [Vec<u8>; 7], base: u32) -> [Vec<u8>; 7] {
    // literal word save: low halves only, outgoing nt cleared, no static field changes.
    let words: [(u32, u16); 14] = [
        (14, 0x2001),
        (16, 0x0fd7),
        (18, 0x5678),
        (20, 0x6789),
        (22, 0x789a),
        (24, 0x89ab),
        (26, 0x7000),
        (28, 0x9abc),
        (30, 0xabcd),
        (32, 0xbcde),
        (34, 0x23),
        (36, 0x1b),
        (38, 0x23),
        (40, 0x23),
    ];
    for (offset, value) in words {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
            let address = base.wrapping_add(offset).wrapping_add(index as u32);
            let page = PAGES
                .iter()
                .position(|page| *page == address & !4095)
                .unwrap();
            before[page][(address & 4095) as usize] = byte;
        }
    }
    before
}
fn expected_ram(before: [Vec<u8>; 7], base: u32, format: TaskFormat) -> [Vec<u8>; 7] {
    match format {
        TaskFormat::Word => saved_word_ram(before, base),
        TaskFormat::Dword => saved_ram(before, base),
    }
}
fn header(bytes: &mut [u8], magic: [u8; 4], version: u16) {
    bytes.fill(0);
    bytes[..4].copy_from_slice(&magic);
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
    let length = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
}
fn arena_after(
    mut before: Vec<u8>,
    state: State32,
    segments: Option<([u16; 6], Option<[u32; 6]>)>,
) -> Vec<u8> {
    let version = if state.eflags & 0x10000 != 0 {
        3
    } else if state.eflags & !0xcd7 != 0 {
        2
    } else {
        1
    };
    header(&mut before[..56], *b"R3ST", version);
    for (index, value) in state.registers.into_iter().enumerate() {
        before[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    before[48..52].copy_from_slice(&state.eip.to_le_bytes());
    before[52..56].copy_from_slice(&state.eflags.to_le_bytes());
    if let Some((selectors, caches)) = segments {
        let bytes = &mut before[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64];
        header(bytes, *b"R3SG", if caches.is_some() { 3 } else { 1 });
        for (index, value) in selectors.into_iter().enumerate() {
            bytes[16 + index * 2..18 + index * 2].copy_from_slice(&value.to_le_bytes());
        }
        if let Some(caches) = caches {
            for (offset, value) in [28, 32, 36, 40, 44, 56].into_iter().zip(caches) {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        header(
            &mut before[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40],
            *b"R3SB",
            1,
        );
    }
    before
}
fn initial_state(format: TaskFormat) -> State32 {
    match format {
        TaskFormat::Word => State32 {
            registers: WORD_GPR,
            eip: 0x2000,
            eflags: 0x4fd7,
        },
        TaskFormat::Dword => State32 {
            registers: INCOMING_GPR,
            eip: 0x2000,
            eflags: 0x250fd7,
        },
    }
}
fn selectors(format: TaskFormat) -> [u16; 6] {
    match format {
        TaskFormat::Word => [0x23, 0x1b, 0x23, 0x23, 0, 0],
        TaskFormat::Dword => [0x23, 0x1b, 0x23, 0x23, 0x3b, 0],
    }
}
fn assert_controls(engine: &EngineInstance, format: TaskFormat) {
    assert_eq!(engine.task_return_sources.unwrap().current.format(), format);
    assert_eq!(engine.task_return_sources.unwrap().current.selector(), 0x4b);
    assert_eq!(engine.task_execution_revision(), Some(2));
    let controls = engine.task_control_state().unwrap().unwrap();
    assert_eq!(
        (
            controls.cr0(),
            controls.cr3(),
            controls.ldtr(),
            controls.dr7()
        ),
        (0x3b, 0xdead_b123, 0, 0x400)
    );
    assert_eq!(controls.nmi_blocked(), Some(false));
}

#[test]
fn word_and_dword_current_target_pairs_use_independent_save_load_formats_on_four_routes() {
    for resident in [false, true] {
        for entries in [false, true] {
            for current in [TaskFormat::Word, TaskFormat::Dword] {
                for target in [TaskFormat::Word, TaskFormat::Dword] {
                    let (mut engine, id) =
                        instance_formats(resident, entries, 0x5000, 0x6000, [current, target]);
                    let before = memory_image(&engine);
                    let arena = engine.arena().to_vec();
                    let version = engine.memory().unwrap().code_validation_version();
                    let result = committed(invoke(&mut engine, id));
                    let state = initial_state(target);
                    assert_eq!(result.context.state(), state);
                    assert_eq!(result.context.selectors(), selectors(target));
                    assert_eq!(result.context.loaded_mask(), 0x3f);
                    assert_eq!(result.context.fault(), None);
                    assert_eq!(result.context.task_trap(), target == TaskFormat::Dword);
                    assert_eq!(
                        result.context.segment_image().caches[4].usable(),
                        target == TaskFormat::Dword
                    );
                    assert!(!result.context.segment_image().caches[5].usable());
                    assert!(result.normal_segments_published);
                    assert_eq!(result.currentness, Ok(StoreCompletion::Complete));
                    assert_eq!(memory_image(&engine), expected_ram(before, 0x5000, current));
                    assert_eq!(
                        engine.memory().unwrap().code_validation_version(),
                        version + 1
                    );
                    assert_eq!(
                        engine.arena(),
                        arena_after(arena, state, Some((selectors(target), None)))
                    );
                    assert_eq!(
                        engine
                            .segment_descriptors
                            .raw_descriptor(0x40)
                            .unwrap()
                            .descriptor_type(),
                        if current == TaskFormat::Word { 1 } else { 9 }
                    );
                    assert_eq!(
                        engine
                            .segment_descriptors
                            .raw_descriptor(0x48)
                            .unwrap()
                            .descriptor_type(),
                        if target == TaskFormat::Word { 3 } else { 11 }
                    );
                    assert_controls(&engine, target);
                    match id {
                        None => assert_eq!(
                            engine.guard_material(KEY, engine.generation()),
                            Err(HostError::InvalidArtifact)
                        ),
                        Some(id) => assert_eq!(
                            engine.guard_material_resident(KEY, id),
                            Err(HostError::InvalidArtifact)
                        ),
                    }
                }
            }
        }
    }
}

#[test]
fn word_target_has_exact_44_byte_read_and_independent_cs_d_ss_b() {
    for resident in [false, true] {
        for format in [TaskFormat::Word, TaskFormat::Dword] {
            let target = if format == TaskFormat::Word {
                0x6fd4
            } else {
                0x6f98
            };
            let (mut engine, id) =
                instance_formats(resident, false, 0x5000, target, [TaskFormat::Dword, format]);
            engine.protect(0x7000, 1, 0).unwrap();
            let version = engine.memory().unwrap().code_validation_version();
            let result = committed(invoke(&mut engine, id));
            assert_eq!(result.context.state(), initial_state(format));
            assert_eq!(result.context.fault(), None);
            assert_eq!(
                engine.memory().unwrap().code_validation_version(),
                version + 1
            );
        }
        let (mut engine, id) =
            instance_formats(resident, true, 0x5000, 0x6000, [TaskFormat::Word; 2]);
        engine
            .segment_descriptors
            .configure_raw(0x18, SegmentDescriptorRecord::from_words(0xffff, 0xfb00))
            .unwrap();
        engine
            .segment_descriptors
            .configure_raw(0x20, SegmentDescriptorRecord::from_words(0xffff, 0xf300))
            .unwrap();
        let arena = engine.arena().to_vec();
        let before = memory_image(&engine);
        let result = committed(invoke(&mut engine, id));
        assert_eq!(result.context.fault(), None);
        assert!(!result.context.segment_image().caches[1].default32());
        assert!(!result.context.segment_image().caches[2].default32());
        assert_eq!(result.context.state(), initial_state(TaskFormat::Word));
        assert_eq!(memory_image(&engine), saved_word_ram(before, 0x5000));
        assert_eq!(
            engine.arena(),
            arena_after(
                arena,
                initial_state(TaskFormat::Word),
                Some((
                    selectors(TaskFormat::Word),
                    Some([0xf030ffff, 0xf0b0ffff, 0xf030ffff, 0xf030ffff, 0, 0])
                ))
            )
        );
    }
}

#[test]
fn word_self_return_reads_saved_low_halves_and_preserves_static_tail() {
    for resident in [false, true] {
        let (mut engine, id) =
            instance_formats(resident, true, 0x6000, 0x6000, [TaskFormat::Word; 2]);
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(0x6000), &0x43_u16.to_le_bytes())
            .unwrap();
        let before = memory_image(&engine);
        let arena = engine.arena().to_vec();
        let version = engine.memory().unwrap().code_validation_version();
        let result = committed(invoke(&mut engine, id));
        let state = State32 {
            registers: [
                0x5678, 0x6789, 0x789a, 0x89ab, 0x7000, 0x9abc, 0xabcd, 0xbcde,
            ],
            eip: 0x2001,
            eflags: 0x0fd7,
        };
        assert_eq!(result.context.state(), state);
        assert_eq!(result.context.fault(), None);
        assert_eq!(result.context.selectors(), selectors(TaskFormat::Word));
        assert_eq!(memory_image(&engine), saved_word_ram(before, 0x6000));
        assert_eq!(
            engine.arena(),
            arena_after(arena, state, Some((selectors(TaskFormat::Word), None)))
        );
        assert_eq!(
            engine.memory().unwrap().code_validation_version(),
            version + 1
        );
        assert_eq!(engine.task_return_sources.unwrap().current.selector(), 0x43);
        assert_eq!(
            engine
                .task_return_sources
                .unwrap()
                .current
                .descriptor()
                .descriptor_type(),
            3
        );
        assert_eq!(
            engine
                .segment_descriptors
                .raw_descriptor(0x40)
                .unwrap()
                .descriptor_type(),
            1
        );
    }
}

#[test]
fn mixed_format_wrapping_overlap_uses_all_outgoing_bytes_before_target_decode() {
    for (current, target, base, expected) in [
        (
            TaskFormat::Word,
            TaskFormat::Dword,
            0xffff_ffe0,
            initial_state(TaskFormat::Dword),
        ),
        (
            TaskFormat::Dword,
            TaskFormat::Word,
            0xffff_ffc0,
            State32 {
                registers: [0x3344, 0x23, 0xbbcc, 0x3b, 0x9bdf, 0, 0xbeef, 0xbabe],
                eip: 0x2000,
                eflags: 3,
            },
        ),
    ] {
        for resident in [false, true] {
            let (mut engine, id) = instance_formats(resident, false, base, 0, [current, target]);
            let before = memory_image(&engine);
            let arena = engine.arena().to_vec();
            let version = engine.memory().unwrap().code_validation_version();
            let result = committed(invoke(&mut engine, id));
            assert_eq!(result.context.state(), expected);
            assert_eq!(result.context.fault(), None);
            assert_eq!(result.context.selectors(), selectors(target));
            assert_eq!(memory_image(&engine), expected_ram(before, base, current));
            assert_eq!(
                engine.arena(),
                arena_after(arena, expected, Some((selectors(target), None)))
            );
            assert_eq!(
                engine.memory().unwrap().code_validation_version(),
                version + 1
            );
            assert_controls(&engine, target);
        }
    }
}

#[test]
fn word_late_save_fault_is_atomic_and_word_postload_fault_retains_new_context() {
    for resident in [false, true] {
        let (mut engine, id) =
            instance_formats(resident, true, 0x5fd7, 0x7000, [TaskFormat::Word; 2]);
        engine.protect(0x6000, 1, 1).unwrap();
        let before = memory_image(&engine);
        let arena = engine.arena().to_vec();
        let version = engine.memory().unwrap().code_validation_version();
        assert_eq!(
            invoke(&mut engine, id),
            TaskReturnTransition32::NeedsPagingAuthority {
                fault: MemoryFault {
                    address: GuestAddress(0x6000),
                    access: Access::Write,
                    reason: FaultReason::Permission
                },
                length: 2
            }
        );
        assert_eq!(memory_image(&engine), before);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        assert_eq!(engine.task_execution_revision(), Some(1));
        assert_eq!(engine.task_incoming_context().unwrap(), None);
        let (mut engine, id) = instance_formats(
            resident,
            false,
            0x5000,
            0x6000,
            [TaskFormat::Dword, TaskFormat::Word],
        );
        engine
            .segment_descriptors
            .configure_raw(
                0x18,
                SegmentDescriptorRecord::from_words(0xffff, 0x00cf7b00),
            )
            .unwrap();
        let before = memory_image(&engine);
        let arena = engine.arena().to_vec();
        let version = engine.memory().unwrap().code_validation_version();
        let result = committed(invoke(&mut engine, id));
        assert_eq!(result.context.state(), initial_state(TaskFormat::Word));
        assert_eq!(result.context.selectors(), selectors(TaskFormat::Word));
        assert_eq!(
            result.context.fault(),
            Some(TaskTransitionFault32::NotPresent(0x18))
        );
        assert_eq!(result.context.loaded_mask(), 4);
        assert!(!result.normal_segments_published);
        assert_eq!(
            engine.task_incoming_context().unwrap(),
            Some(result.context)
        );
        assert_eq!(
            engine.arena(),
            arena_after(arena, initial_state(TaskFormat::Word), None)
        );
        assert_eq!(memory_image(&engine), saved_ram(before, 0x5000));
        assert_eq!(
            engine.memory().unwrap().code_validation_version(),
            version + 1
        );
        assert_controls(&engine, TaskFormat::Word);
    }
}
