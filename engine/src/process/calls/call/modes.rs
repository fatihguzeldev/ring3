use super::*;
use crate::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::{SegmentAccess, SegmentAccessError, SegmentCache, SegmentContext, SegmentRegister},
};

#[test]
fn flat_host_call_rejects_d16_before_stack_reads_token_or_publication() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    let mut context = SegmentContext::default();
    context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
    for (index, value) in [0x1000_u32, 2, 0x1000, 77].into_iter().enumerate() {
        arena[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8000; // deliberately unmapped; the mode barrier must precede stack RAM.
    encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 77 },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.capture_call(91, generation, CallingConvention32::Cdecl, 0),
        Err(HostError::Call(CallError::InvalidRequest))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.call_token, 0);
    assert!(engine.pending_call.is_none());
}

fn bridge_with_stack(context: SegmentContext) -> (EngineInstance, u32) {
    assert!(context.is_valid());
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    engine.write32(0x8000, 0x2000).unwrap();
    engine.write32(0x8004, 0x1234_5678).unwrap();
    let mut segments = [0; SEGMENTS_SIZE];
    let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
    for (index, value) in [0x1000_u32, 2, 0x1000, 77].into_iter().enumerate() {
        arena[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8000;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 77 },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    (engine, generation)
}

fn assert_bridge_rejected(context: SegmentContext) {
    let (mut engine, generation) = bridge_with_stack(context);
    let before = engine.arena().to_vec();
    let mut stack_before = [0; 8];
    engine
        .memory()
        .unwrap()
        .read(crate::memory::GuestAddress(0x8000), &mut stack_before)
        .unwrap();
    assert_eq!(
        engine.capture_call(91, generation, CallingConvention32::Cdecl, 1),
        Err(HostError::Call(CallError::InvalidRequest))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.call_token, 0);
    assert!(engine.pending_call.is_none());
    let mut stack_after = [0; 8];
    engine
        .memory()
        .unwrap()
        .read(crate::memory::GuestAddress(0x8000), &mut stack_after)
        .unwrap();
    assert_eq!(stack_after, stack_before);
}

#[test]
fn flat_host_call_rejects_bounded_stack_before_argument_reads_or_publication() {
    let mut context = SegmentContext::default();
    let stack = SegmentCache::from_bits(0xf230_8003).unwrap();
    assert_eq!(stack.check_span(0x8000, 4, SegmentAccess::Read), Ok(()));
    assert_eq!(
        stack.check_span(0x8004, 4, SegmentAccess::Read),
        Err(SegmentAccessError::Limit)
    );
    context.caches[SegmentRegister::Ss.index()] = stack;
    assert_bridge_rejected(context);
}

#[test]
fn flat_host_call_rejects_expand_down_stack_before_token_or_publication() {
    let mut context = SegmentContext::default();
    let stack = SegmentCache::from_bits(0xf270_7fff).unwrap();
    assert_eq!(stack.check_span(0x8000, 8, SegmentAccess::Read), Ok(()));
    assert_eq!(
        stack.check_span(0x7ffc, 4, SegmentAccess::Write),
        Err(SegmentAccessError::Limit)
    );
    context.caches[SegmentRegister::Ss.index()] = stack;
    assert_bridge_rejected(context);
}

#[test]
fn flat_host_call_rejects_full_limit_expand_down_stack_before_reads_or_publication() {
    let mut context = SegmentContext::default();
    let stack = SegmentCache::from_bits(0xf37f_ffff).unwrap();
    assert_eq!(stack.limit(), u32::MAX);
    assert_eq!(
        stack.check_span(0x8000, 8, SegmentAccess::Read),
        Err(SegmentAccessError::Limit)
    );
    context.caches[SegmentRegister::Ss.index()] = stack;
    assert_bridge_rejected(context);
}

#[test]
fn flat_host_call_rejects_bounded_code_before_capturing_an_out_of_limit_return() {
    let mut context = SegmentContext::default();
    let code = SegmentCache::from_bits(0xf2b0_1fff).unwrap();
    assert_eq!(code.check_span(0x1000, 2, SegmentAccess::Execute), Ok(()));
    assert_eq!(
        code.check_span(0x2000, 1, SegmentAccess::Execute),
        Err(SegmentAccessError::Limit)
    );
    context.caches[SegmentRegister::Cs.index()] = code;
    assert_bridge_rejected(context);
}

#[test]
fn flat_host_call_keeps_bounded_data_and_thread_segments() {
    let mut context = SegmentContext::default();
    for segment in [SegmentRegister::Ds, SegmentRegister::Fs] {
        context.caches[segment.index()] = SegmentCache::from_bits(0xf230_00ff).unwrap();
    }
    context.state.bases[SegmentRegister::Fs.index()] = 0x4000;
    let (mut engine, generation) = bridge_with_stack(context);
    let record = engine
        .capture_call(91, generation, CallingConvention32::Cdecl, 1)
        .unwrap();
    assert_eq!(record.return_pc, 0x2000);
    assert_eq!(record.arguments[0], 0x1234_5678);
    engine
        .complete_call(91, generation, record.token, 9)
        .unwrap();
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (state.eip, state.registers[0], state.registers[4]),
        (0x2000, 9, 0x8004)
    );
}
