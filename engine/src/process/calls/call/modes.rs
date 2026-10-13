use super::*;
use crate::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::{SegmentCache, SegmentContext},
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
