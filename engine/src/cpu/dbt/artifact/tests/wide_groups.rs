use super::{
    group_tests::{memory, prepared, resident},
    *,
};

#[test]
fn wide_catalog_appends_after_legacy_ordinals_and_disambiguates_overlapping_blocks() {
    let memory = memory();
    let mut first = prepared(&memory, 0x1000, 32);
    first
        .blocks
        .append(&mut prepared(&memory, 0x1000, 9).blocks);
    first
        .blocks
        .append(&mut prepared(&memory, 0x2000, 16).blocks);
    let region = CompiledRegion::new(
        first,
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap();
    assert_eq!(region.x87_groups.len(), 10);
    for ordinal in 0..7 {
        assert_eq!(region.x87_group(ordinal).unwrap().len(), 8);
        assert!(region.x87_group32(ordinal).is_none());
    }
    for (ordinal, pc, length) in [(7, 0x1000, 32), (8, 0x1000, 9), (9, 0x2000, 16)] {
        assert!(region.x87_group(ordinal).is_none());
        let view = region.x87_group32(ordinal).unwrap();
        assert_eq!(view.len(), length);
        assert_eq!(view.descriptor(0).unwrap().pc, pc);
        assert_eq!(
            view.descriptor(length - 1).unwrap().next_pc,
            pc + length as u32 * 2
        );
        assert!(view.descriptor(length).is_none());
    }
    assert!(region.x87_group32(10).is_none());
    assert!(region.x87_group32(u32::MAX).is_none());
    assert_eq!(std::mem::size_of::<X87GroupSpan32>(), 6);
}

#[test]
fn wide_metadata_never_retains_trial_prefixes_or_replacement_descriptors() {
    let mut memory = memory();
    let cs = CodeFetchContext::from_segments(&crate::cpu::x86::SegmentContext::default());
    for binding in [
        None,
        Some(wasm::EmbeddedBinding::Replacement {
            key: 7,
            generation: 1,
        }),
    ] {
        let region = CompiledRegion::new(
            prepared(&memory, 0x1000, 32),
            Vec::new(),
            super::super::CompilationTier32::Baseline,
            binding,
        )
        .unwrap();
        assert_eq!(region.x87_groups.capacity(), 0);
        assert!(region.x87_group32(0).is_none());
    }
    let minimum = prepared(&memory, 0x1000, 1);
    let bytes = wasm::emit(&minimum.blocks, resident(), minimum.decode_key).len();
    let region = emit_entry_prepared(
        &memory,
        prepared(&memory, 0x1000, 32),
        CompileLimits {
            wasm_bytes: bytes,
            ..CompileLimits::default()
        },
        resident(),
        cs,
    )
    .unwrap();
    assert!(region.metadata().instructions < 9);
    assert!(region.x87_groups.iter().all(|span| span.length <= 8));
    for ordinal in 0..=region.x87_groups.len() {
        assert!(region.x87_group32(ordinal as u32).is_none());
    }
    assert!(region.wasm_bytes(&memory).unwrap().len() <= bytes);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert_eq!(
        region.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
}
