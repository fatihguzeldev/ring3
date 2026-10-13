use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE},
    },
    process::EngineInstance,
};

// preparation reads the configured CS key/limit; preserve that input authority
// while the caller dirties unrelated state to test whole-arena neutrality.
pub fn poison_preserving_code_context(engine: &mut EngineInstance, poison: impl FnOnce(&mut [u8])) {
    let segments = engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].to_vec();
    let bases = engine.arena()
        [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .to_vec();
    let arena = engine.arena_mut().unwrap();
    poison(arena);
    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&bases);
}
