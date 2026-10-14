use super::*;
use crate::{
    abi::{
        arena::{STATE_OFFSET, TRANSFER_OFFSET},
        tasks::{TASK_CONTEXT_SIZE, TaskContextRecord32, decode_task_context},
        x86::{STATE_SIZE, encode_state_current},
    },
    cpu::x86::{
        SegmentCache, StackFrameKind, State32,
        tasks::{TaskContextImage32, TaskIncomingContext32, TaskSave},
    },
    process::cpu::tasks::TaskReturnTransition32,
};

fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(3, 7).unwrap();
    for page in [0x4000, 0x5000, 0x7000] {
        e.map(page, 1, 7).unwrap();
    }
    for (selector, base) in [(0x48, 0x5000), (0x50, 0x5000)] {
        e.configure_segment_descriptor_raw(
            selector,
            SegmentDescriptorRecord::from_words((base << 16) | 0xffff, 0x0040_f200),
        )
        .unwrap();
    }
    let mut segments = SegmentContext::default();
    segments.state.selectors = [0x23, 0x1b, 0x23, 0x4b, 0x53, 0];
    segments.state.bases[1] = 0x3000;
    segments.state.bases[2] = 0x7000;
    segments.caches[1] = SegmentCache::from_bits(0xf0bf_ffff).unwrap();
    segments.caches[2] = SegmentCache::from_bits(0xf030_ffff).unwrap();
    for index in [3, 4, 5] {
        segments.caches[index] = SegmentCache::UNUSABLE;
    }
    let mut state = State32 {
        eip: 0x1000,
        eflags: 0x4002,
        ..State32::default()
    };
    state.registers[4] = 0xabcd_0010;
    let incoming = TaskIncomingContext32::from_image(TaskContextImage32 {
        state,
        segments,
        loaded: 0x27,
        ldtr: 0,
        ldt_cache: None,
        task_trap: false,
        fault: None,
    })
    .unwrap();
    e.task_execution_revision = 1;
    e.task_incoming = Some(incoming);
    encode_state_current(
        &state,
        &mut e.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x4000), &[0xeb, 0xfe])
        .unwrap();
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    e.compile(1).unwrap();
    e
}
#[test]
fn partial_task_operands_use_only_qualified_raw_caches_and_segment_load_repairs_canonically() {
    let mut e = engine();
    let old = e.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].to_vec();
    assert_eq!(e.segment_context(), Err(HostError::InvalidRequest));
    let raw = e.operand_segment_context().unwrap();
    assert_eq!(raw.state.selectors[3], 0x4b);
    assert_eq!(raw.state.bases[2], 0x7000);
    assert!(!raw.caches[2].default32());
    assert_eq!(
        check_cached_span(&raw, SegmentRegister::Fs, 0, 1, SegmentAccess::Read),
        Err(CpuMemoryError::GeneralProtection(0))
    );
    assert_eq!(
        check_cached_span(&raw, SegmentRegister::Ss, 0, 2, SegmentAccess::Write),
        Ok(())
    );
    e.segment_load(3, 0x4b).unwrap();
    assert_eq!(e.task_incoming.unwrap().loaded_mask(), 0x2f);
    assert_eq!(
        &e.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
        old
    );
    assert_eq!(e.operand_segment_context().unwrap().state.bases[3], 0x5000);
    e.read_task_context().unwrap();
    let TaskContextRecord32::Loaded { context, .. } =
        decode_task_context(&e.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE])
            .unwrap()
    else {
        panic!("expected raw partial record")
    };
    assert_eq!(context.loaded_mask(), 0x2f);
    assert_eq!(context.selectors()[4], 0x53);
    e.segment_load(4, 0).unwrap();
    let complete = e.segment_context().unwrap();
    assert_eq!(e.task_incoming.unwrap().loaded_mask(), 0x3f);
    assert_eq!(complete.state.selectors, [0x23, 0x1b, 0x23, 0x4b, 0, 0]);
    assert_eq!(complete.state.bases, [0, 0x3000, 0x7000, 0x5000, 0, 0]);
    assert!(!complete.caches[1].default32());
    assert!(!complete.caches[2].default32());
    assert_eq!(complete.caches[4], SegmentCache::UNUSABLE);
    e.read_task_context().unwrap();
    assert!(
        matches!(decode_task_context(&e.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET+TASK_CONTEXT_SIZE]),
        Ok(TaskContextRecord32::Loaded{context,..}) if context.validated_segments()==Some(complete))
    );
}
#[test]
fn partial_task_stack_and_nested_save_use_new_ss_b_and_raw_selectors() {
    let mut e = engine();
    let input = [1, 2, 3, 4, 0xabcd_0010, 6, 7, 8];
    for (index, value) in input.into_iter().enumerate() {
        e.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&u32::to_le_bytes(value));
    }
    e.stack_execute(StackFrameKind::PushAll16.control(0, 0))
        .unwrap();
    let mut ram = [0; 16];
    e.memory()
        .unwrap()
        .read(GuestAddress(0x7000), &mut ram)
        .unwrap();
    assert_eq!(ram, [8, 0, 7, 0, 6, 0, 0x10, 0, 4, 0, 3, 0, 2, 0, 1, 0]);
    let raw = e.task_incoming.unwrap();
    let outgoing = TaskSave {
        next_eip: 0x1002,
        flags: 0x4002,
        general: input,
        selectors: raw.selectors(),
    };
    assert!(matches!(
        e.task_return_transition(outgoing),
        Ok(TaskReturnTransition32::NeedsAuthority(
            crate::cpu::x86::tasks::TaskAuthorityRequirement32::Sources
        ))
    ));
    let old = TaskSave {
        selectors: SegmentContext::default().state.selectors,
        ..outgoing
    };
    assert_eq!(
        e.task_return_transition(old),
        Err(HostError::InvalidRequest)
    );
}

#[test]
fn segment_authority_query_does_not_decode_unrelated_live_general_state() {
    let mut e = engine();
    let expected = e.task_incoming.unwrap().segment_image();
    e.arena_mut().unwrap()[STATE_OFFSET] = b'X';
    assert_eq!(e.operand_segment_context(), Ok(expected));
    assert_eq!(e.task_context_snapshot(), Err(HostError::Infrastructure));
    e.arena_mut().unwrap()[STATE_OFFSET] = b'R';
    e.segment_load(3, 0x4b).unwrap();
    e.segment_load(4, 0).unwrap();
    let complete = e.segment_context().unwrap();
    e.arena_mut().unwrap()[STATE_OFFSET] = b'X';
    assert_eq!(e.operand_segment_context(), Ok(complete));
    e.close();
    assert_eq!(e.operand_segment_context(), Err(HostError::Closed));
}
