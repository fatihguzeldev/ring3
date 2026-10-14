use super::*;
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{decode_state, encode_state_current},
    },
    cpu::x86::{SegmentContext, SegmentDescriptorRecord, State32, tasks::TaskRegister},
    memory::{Access, FaultReason},
};

const KEY: u64 = 0x5453_5452_414e_5331;
const PAGES: [u32; 7] = [0, 0x1000, 0x2000, 0x5000, 0x6000, 0x7000, 0xffff_f000];
const INCOMING_GPR: [u32; 8] = [
    0x11223344, 0x55667788, 0x99aabbcc, 0xddeeff00, 0x13579bdf, 0x2468ace0, 0xdeadbeef, 0xcafebabe,
];

fn outgoing() -> TaskSave {
    TaskSave {
        next_eip: 0x2001,
        flags: 0x254fd7,
        general: [
            0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x7000, 0x56789abc, 0x6789abcd,
            0x789abcde,
        ],
        selectors: [0x23, 0x1b, 0x23, 0x23, 0x3b, 0],
    }
}
fn tss(base: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | 0x67,
        (base & 0xff000000) | ((base >> 16) & 0xff) | 0x8b00,
    )
}
fn incoming_bytes() -> [u8; 104] {
    let mut bytes = [0xa5; 104];
    bytes[28..32].copy_from_slice(&0xcccc_d000_u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&0x2000_u32.to_le_bytes());
    bytes[36..40].copy_from_slice(&0x250fd7_u32.to_le_bytes());
    for (index, value) in INCOMING_GPR.into_iter().enumerate() {
        bytes[40 + index * 4..44 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    for (index, value) in [0x23_u16, 0x1b, 0x23, 0x23, 0x3b, 0]
        .into_iter()
        .enumerate()
    {
        bytes[72 + index * 4..74 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[96..98].copy_from_slice(&0_u16.to_le_bytes());
    bytes[100..102].copy_from_slice(&1_u16.to_le_bytes());
    bytes[102..104].copy_from_slice(&104_u16.to_le_bytes());
    bytes
}
fn memory_image(engine: &EngineInstance) -> [Vec<u8>; 7] {
    PAGES.map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    })
}
fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Option<u64> {
    let descriptor: &[u32] = if entries { &[0x2000] } else { &[0x2000, 2] };
    for (index, value) in descriptor.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    match (resident, entries) {
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
    }
}
fn instance(
    resident: bool,
    entries: bool,
    current: u32,
    target: u32,
) -> (EngineInstance, Option<u64>) {
    let mut engine = EngineInstance::new(7, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(page), &[0xa5; 4096])
            .unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(target), &incoming_bytes())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(current), &0x4b_u16.to_le_bytes())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(0x2000), &[0xeb, 0xfe])
        .unwrap();
    let current = TaskRegister::from_cached(0x43, tss(current)).unwrap();
    engine.configure_task_return_sources(current, 0x57).unwrap();
    engine
        .configure_segment_descriptor_raw(0x40, current.descriptor())
        .unwrap();
    engine
        .configure_segment_descriptor_raw(0x48, tss(target))
        .unwrap();
    engine
        .configure_task_controls(
            TaskControlState32::new(0x33, 0xdead_b123, 0, 0x400, Some(true)).unwrap(),
        )
        .unwrap();
    let id = compile(&mut engine, resident, entries);
    let save = outgoing();
    encode_state_current(
        &State32 {
            registers: save.general,
            eip: 0x2000,
            eflags: save.flags,
        },
        &mut engine.arena_mut().unwrap()[..56],
    )
    .unwrap();
    (engine, id)
}
fn invoke(engine: &mut EngineInstance, id: Option<u64>) -> TaskReturnTransition32 {
    match id {
        None => engine.task_return_transition(outgoing()),
        Some(id) => engine.task_return_transition_resident(KEY, id, outgoing()),
    }
    .unwrap()
}
fn saved_ram(mut before: [Vec<u8>; 7], base: u32) -> [Vec<u8>; 7] {
    // literal outgoing dynamic image: nt/rf clear; all static and reserved bytes remain untouched.
    let words: [(u16, u16); 26] = [
        (32, 0x2001),
        (34, 0),
        (36, 0x0fd7),
        (38, 0x24),
        (40, 0x5678),
        (42, 0x1234),
        (44, 0x6789),
        (46, 0x2345),
        (48, 0x789a),
        (50, 0x3456),
        (52, 0x89ab),
        (54, 0x4567),
        (56, 0x7000),
        (58, 0),
        (60, 0x9abc),
        (62, 0x5678),
        (64, 0xabcd),
        (66, 0x6789),
        (68, 0xbcde),
        (70, 0x789a),
        (72, 0x23),
        (76, 0x1b),
        (80, 0x23),
        (84, 0x23),
        (88, 0x3b),
        (92, 0),
    ];
    for (offset, value) in words {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
            let address = base
                .wrapping_add(u32::from(offset))
                .wrapping_add(index as u32);
            let page = PAGES
                .iter()
                .position(|page| *page == address & !4095)
                .unwrap();
            before[page][(address & 4095) as usize] = byte;
        }
    }
    before
}
fn committed(value: TaskReturnTransition32) -> CommittedTaskReturn32 {
    let TaskReturnTransition32::Committed(value) = value else {
        panic!("expected committed task, got {value:?}")
    };
    value
}
fn assert_saved_commit(engine: &EngineInstance, before: [Vec<u8>; 7], version: u64, base: u32) {
    assert_eq!(memory_image(engine), saved_ram(before, base));
    assert_eq!(
        engine.memory().unwrap().code_validation_version(),
        version + 1
    );
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x40)
            .unwrap()
            .descriptor_type(),
        9
    );
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x48)
            .unwrap()
            .descriptor_type(),
        11
    );
    assert_eq!(engine.task_return_sources.unwrap().current.selector(), 0x4b);
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
    assert_eq!(engine.task_execution_revision(), Some(2));
}

#[test]
fn supplied_pg0_task32_commits_save_live_controls_and_same_cs_revision_on_all_four_routes() {
    for resident in [false, true] {
        for entries in [false, true] {
            let (mut engine, id) = instance(resident, entries, 0x5000, 0x6000);
            let before = memory_image(&engine);
            let version = engine.memory().unwrap().code_validation_version();
            let mut arena = engine.arena().to_vec();
            let result = committed(invoke(&mut engine, id));
            assert_eq!(
                result.context.state(),
                State32 {
                    registers: INCOMING_GPR,
                    eip: 0x2000,
                    eflags: 0x250fd7
                }
            );
            assert_eq!(result.context.fault(), None);
            assert_eq!(result.context.loaded_mask(), 0x3f);
            assert_eq!(
                result.context.validated_segments(),
                Some(SegmentContext::default())
            );
            assert!(result.context.task_trap());
            assert!(result.normal_segments_published);
            assert_eq!(result.currentness, Ok(StoreCompletion::Complete));
            encode_state_current(
                &State32 {
                    registers: INCOMING_GPR,
                    eip: 0x2000,
                    eflags: 0x250fd7,
                },
                &mut arena[..56],
            )
            .unwrap();
            assert_eq!(engine.arena(), arena);
            assert_saved_commit(&engine, before, version, 0x5000);
            assert_eq!(engine.segment_context().unwrap(), SegmentContext::default());
            match id {
                Some(id) => {
                    assert_eq!(
                        engine.guard_resident(KEY, id),
                        Err(HostError::InvalidArtifact)
                    );
                    assert_eq!(
                        engine.guard_material_resident(KEY, id),
                        Err(HostError::InvalidArtifact)
                    );
                    assert!(matches!(
                        engine.lookup_resident(0x2000),
                        Err(HostError::Resident(
                            crate::cpu::dbt::RegistryError::NotFound { .. }
                        ))
                    ));
                }
                None => {
                    assert_eq!(
                        engine.guard(KEY, engine.generation()),
                        Err(HostError::InvalidArtifact)
                    );
                    assert_eq!(
                        engine.guard_material(KEY, engine.generation()),
                        Err(HostError::InvalidArtifact)
                    );
                }
            }
            let next = compile(&mut engine, resident, entries);
            match next {
                Some(id) => assert_eq!(engine.guard_material_resident(KEY, id), Ok(())),
                None => assert_eq!(engine.guard_material(KEY, engine.generation()), Ok(())),
            }
        }
    }
}

#[test]
fn committed_ss_and_cs_faults_keep_new_raw_state_and_block_old_r3sg_projection() {
    for resident in [false, true] {
        for (ss, expected) in [
            (true, TaskTransitionFault32::StackSegment(0x50)),
            (false, TaskTransitionFault32::NotPresent(0x18)),
        ] {
            let (mut engine, id) = instance(resident, false, 0x5000, 0x6000);
            if ss {
                engine
                    .segment_descriptors
                    .configure_raw(
                        0x50,
                        SegmentDescriptorRecord::from_words(0xffff, 0x00cf7300),
                    )
                    .unwrap();
                engine
                    .memory
                    .as_mut()
                    .unwrap()
                    .cpu_write(GuestAddress(0x6050), &0x53_u16.to_le_bytes())
                    .unwrap();
            } else {
                engine
                    .segment_descriptors
                    .configure_raw(
                        0x18,
                        SegmentDescriptorRecord::from_words(0xffff, 0x00cf7b00),
                    )
                    .unwrap();
            }
            let before = memory_image(&engine);
            let version = engine.memory().unwrap().code_validation_version();
            let mut arena = engine.arena().to_vec();
            let result = committed(invoke(&mut engine, id));
            assert_eq!(result.context.fault(), Some(expected));
            assert_eq!(
                result.context.loaded_mask(),
                if ss {
                    0
                } else {
                    1 << SegmentRegister::Ss.index()
                }
            );
            assert_eq!(result.context.selectors()[2], if ss { 0x53 } else { 0x23 });
            assert_eq!(result.context.state().registers, INCOMING_GPR);
            assert!(!result.normal_segments_published);
            assert_eq!(
                engine.task_incoming_context().unwrap(),
                Some(result.context)
            );
            encode_state_current(&result.context.state(), &mut arena[..56]).unwrap();
            assert_eq!(engine.arena(), arena);
            assert_saved_commit(&engine, before, version, 0x5000);
            assert_eq!(engine.segment_context(), Err(HostError::InvalidRequest));
            assert_eq!(engine.code_decode_key(), Err(HostError::InvalidRequest));
            assert_eq!(engine.compile_entries(1, 0), Err(HostError::InvalidRequest));
        }
    }
}

#[test]
fn self_and_wrapped_overlap_load_the_outgoing_saved_image_before_incoming_state() {
    for (current, target, self_link) in [(0x6000, 0x6000, true), (0xffff_ffc0, 0, false)] {
        let (mut engine, id) = instance(false, true, current, target);
        if self_link {
            engine
                .memory
                .as_mut()
                .unwrap()
                .cpu_write(GuestAddress(current), &0x43_u16.to_le_bytes())
                .unwrap();
        }
        let before = memory_image(&engine);
        let version = engine.memory().unwrap().code_validation_version();
        let result = committed(invoke(&mut engine, id));
        let expected = saved_ram(before.clone(), current);
        let mut literal = [0; 104];
        for (index, byte) in literal.iter_mut().enumerate() {
            let address = target.wrapping_add(index as u32);
            let page = PAGES
                .iter()
                .position(|page| *page == address & !4095)
                .unwrap();
            *byte = expected[page][(address & 4095) as usize];
        }
        assert_eq!(memory_image(&engine), expected);
        assert_eq!(
            engine.memory().unwrap().code_validation_version(),
            version + 1
        );
        assert_eq!(
            result.context.state().eip,
            u32::from_le_bytes(literal[32..36].try_into().unwrap())
        );
        if self_link {
            assert_eq!(result.context.state().eip, 0x2001);
            assert_eq!(result.context.state().eflags, 0x240fd7);
            assert_eq!(result.context.state().registers, outgoing().general);
            assert_eq!(engine.task_return_sources.unwrap().current.selector(), 0x43);
            assert_eq!(
                engine
                    .segment_descriptors
                    .raw_descriptor(0x40)
                    .unwrap()
                    .descriptor_type(),
                9
            );
        }
    }
}

#[test]
fn precommit_fault_and_late_save_permission_failure_do_not_publish_new_task() {
    let (mut engine, id) = instance(false, false, 0x5000, 0x6000);
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(0x5000), &[0, 0])
        .unwrap();
    let before = memory_image(&engine);
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().code_validation_version();
    assert_eq!(
        invoke(&mut engine, id),
        TaskReturnTransition32::PreCommitFault(TaskTransitionFault32::InvalidTss(0))
    );
    assert_eq!(memory_image(&engine), before);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(engine.task_return_sources.unwrap().current.selector(), 0x43);
    assert_eq!(
        engine.task_control_state().unwrap().unwrap().nmi_blocked(),
        Some(false)
    );
    assert_eq!(engine.task_execution_revision(), Some(1));
    let (mut engine, id) = instance(false, true, 0x5fc0, 0x7000);
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
    assert_eq!(engine.task_incoming_context().unwrap(), None);
    assert_eq!(engine.task_execution_revision(), Some(1));
}

#[test]
fn paging_and_missing_live_segment_records_remain_explicit_authority_requirements() {
    let (mut engine, id) = instance(false, false, 0x5000, 0x6000);
    engine.task_controls.as_mut().unwrap().state =
        TaskControlState32::new(0x80000033, 0xdeadb123, 0, 0, None).unwrap();
    let before = memory_image(&engine);
    let arena = engine.arena().to_vec();
    assert_eq!(
        invoke(&mut engine, id),
        TaskReturnTransition32::NeedsAuthority(TaskAuthorityRequirement32::AddressSpace)
    );
    assert_eq!(memory_image(&engine), before);
    assert_eq!(engine.arena(), arena);
    engine.task_controls.as_mut().unwrap().state =
        TaskControlState32::new(0x33, 0xdeadb123, 0, 0, None).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(0x6054), &0x53_u16.to_le_bytes())
        .unwrap();
    let before = memory_image(&engine);
    assert_eq!(
        invoke(&mut engine, id),
        TaskReturnTransition32::NeedsAuthority(TaskAuthorityRequirement32::Descriptor(0x53))
    );
    assert_eq!(memory_image(&engine), before);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.task_execution_revision(), Some(1));
}

#[test]
fn outgoing_save_smc_is_reported_after_ram_and_task_state_commit_without_rollback() {
    for resident in [false, true] {
        let (mut engine, id) = instance(resident, false, 0x1fe0, 0x6000);
        let before = memory_image(&engine);
        let version = engine.memory().unwrap().code_validation_version();
        let result = committed(invoke(&mut engine, id));
        assert_eq!(result.currentness, Ok(StoreCompletion::CodeInvalidated));
        assert_eq!(memory_image(&engine), saved_ram(before, 0x1fe0));
        assert_eq!(
            engine.memory().unwrap().code_validation_version(),
            version + 1
        );
        assert_eq!(engine.task_return_sources.unwrap().current.selector(), 0x4b);
        assert_eq!(
            decode_state(&engine.arena()[..56]).unwrap(),
            result.context.state()
        );
        assert!(
            engine
                .task_control_state()
                .unwrap()
                .unwrap()
                .task_switched()
        );
        assert_eq!(engine.task_execution_revision(), Some(2));
    }
}
