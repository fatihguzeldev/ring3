use super::*;
use crate::cpu::x86::tasks::{TaskState, TaskState32};

fn incoming() -> TaskState32 {
    TaskState32 {
        eip: 0x1234,
        flags: 0x250fd7,
        general: [0x11223344; 8],
        selectors: [0x23, 0x1b, 0x23, 0x23, 0x3b, 0],
        ldtr: 0,
        cr3: 0xdead0000,
        task_trap: true,
        io_map_base: 104,
    }
}
fn record(kind: u8, dpl: u8, present: bool) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        0xffff,
        0x00cf_0000 | (u32::from(kind) << 8) | (u32::from(dpl) << 13) | (u32::from(present) << 15),
    )
}
fn qualify(state: TaskState32, table: &SegmentDescriptorTable) -> TaskIncomingContext32 {
    incoming_segments(
        TaskLoadState32::from_record(TaskState::Dword(state)),
        table,
        0x57,
        (0x40, SegmentDescriptorRecord::from_words(0x67, 0x8900)),
    )
    .unwrap()
}

#[test]
fn raw_incoming_selectors_survive_postcommit_fault_with_cache_progress() {
    let mut table = SegmentDescriptorTable::default();
    let mut state = incoming();
    state.selectors[2] = 0x53;
    table.configure_raw(0x50, record(0x13, 3, false)).unwrap();
    let context = qualify(state, &table);
    assert_eq!(context.state().registers, state.general);
    assert_eq!(context.state().eip, 0x1234);
    assert_eq!(context.state().eflags, 0x250fd7);
    assert_eq!(context.selectors(), [0x23, 0x1b, 0x53, 0x23, 0x3b, 0]);
    assert_eq!(
        context.fault(),
        Some(TaskTransitionFault32::StackSegment(0x50))
    );
    assert_eq!(context.loaded_mask(), 0);
    assert!(context.validated_segments().is_none());
    assert!(context.task_trap());
    state.selectors[2] = 0x22;
    table.configure_raw(0x18, record(0x1b, 3, false)).unwrap();
    let context = qualify(state, &table);
    assert_eq!(
        context.fault(),
        Some(TaskTransitionFault32::NotPresent(0x18))
    );
    assert_eq!(context.loaded_mask(), 0);
    state.selectors[2] = 0x23;
    let valid_ss = qualify(state, &table);
    assert_eq!(
        valid_ss.fault(),
        Some(TaskTransitionFault32::NotPresent(0x18))
    );
    assert_eq!(valid_ss.loaded_mask(), 1 << SegmentRegister::Ss.index());
    state.selectors[2] = 0x22;
    table.configure_raw(0x18, record(0x1b, 3, true)).unwrap();
    let context = qualify(state, &table);
    assert_eq!(
        context.fault(),
        Some(TaskTransitionFault32::InvalidTss(0x20))
    );
    assert_eq!(context.loaded_mask(), 1 << SegmentRegister::Cs.index());
}

#[test]
fn p6_postload_competing_checks_and_null_data_are_explicit() {
    let mut table = SegmentDescriptorTable::default();
    let mut state = incoming();
    state.ldtr = 0x50;
    table.configure_raw(0x50, record(2, 0, false)).unwrap();
    table.configure_raw(0x20, record(0x13, 3, false)).unwrap();
    assert_eq!(
        qualify(state, &table).fault(),
        Some(TaskTransitionFault32::StackSegment(0x20))
    );
    table.configure_raw(0x20, record(0x13, 3, true)).unwrap();
    assert_eq!(
        qualify(state, &table).fault(),
        Some(TaskTransitionFault32::InvalidTss(0x50))
    );
    state.ldtr = 0;
    state.selectors[0] = 0;
    state.selectors[3] = 3;
    state.selectors[4] = 2;
    let context = qualify(state, &table);
    assert_eq!(context.fault(), None);
    assert_eq!(context.loaded_mask(), 0x3f);
    let segments = context.validated_segments().unwrap();
    for register in [
        SegmentRegister::Es,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ] {
        assert_eq!(segments.caches[register.index()], SegmentCache::UNUSABLE);
    }
}

#[test]
fn missing_descriptors_and_ldt_contents_do_not_become_fabricated_ts() {
    let mut state = incoming();
    state.selectors[3] = 0x53;
    let table = SegmentDescriptorTable::default();
    assert_eq!(
        incoming_segments(
            TaskLoadState32::from_record(TaskState::Dword(state)),
            &table,
            0x57,
            (0x40, record(9, 0, true))
        ),
        Err(TaskAuthorityRequirement32::Descriptor(0x53))
    );
    let mut table = table;
    state.ldtr = 0x50;
    table.configure_raw(0x50, record(2, 0, true)).unwrap();
    state.selectors[3] = 0xf;
    assert_eq!(
        incoming_segments(
            TaskLoadState32::from_record(TaskState::Dword(state)),
            &table,
            0x57,
            (0x40, record(9, 0, true))
        ),
        Err(TaskAuthorityRequirement32::Ldt(0x50))
    );
    state.ldtr = 0;
    assert_eq!(
        qualify(state, &table).fault(),
        Some(TaskTransitionFault32::InvalidTss(0xc))
    );
}
