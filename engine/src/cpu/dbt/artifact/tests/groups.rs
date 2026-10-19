use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

pub(super) fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for pc in [0x1000, 0x2000] {
        memory
            .write(GuestAddress(pc), &[0xd9, 0xe8].repeat(64))
            .unwrap();
    }
    memory
}

pub(super) fn prepared(memory: &AddressSpace, pc: u32, instructions: usize) -> PreparedRegion {
    prepare_runtime_entry(
        memory,
        GuestAddress(pc),
        CompileLimits {
            instructions,
            ..CompileLimits::default()
        },
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

pub(super) fn resident() -> Option<wasm::EmbeddedBinding> {
    Some(wasm::EmbeddedBinding::Resident { key: 7, id: 1 })
}

#[test]
fn group_ordinals_identify_overlapping_blocks_with_different_lengths() {
    let memory = memory();
    let mut first = prepared(&memory, 0x1000, 8);
    let mut overlap = prepared(&memory, 0x1000, 4);
    let mut later = prepared(&memory, 0x2000, 5);
    first.blocks.append(&mut overlap.blocks);
    first.blocks.append(&mut later.blocks);
    // metadata construction sees the final owned ir; repeated first pcs must not
    // select another block's group through a pc-only binary search.
    let region = CompiledRegion::new(
        first,
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap();
    for (ordinal, pc, length) in [(0, 0x1000, 8), (1, 0x1000, 4), (2, 0x2000, 5)] {
        let group = region.x87_group(ordinal).unwrap();
        assert_eq!(group.len(), length);
        assert_eq!(group.steps()[0].pc, pc);
        assert_eq!(
            group.steps().last().unwrap().next_pc,
            pc + length as u32 * 2
        );
    }
    assert!(region.x87_group(3).is_none());
    assert!(region.x87_group(u32::MAX).is_none());
    assert_eq!(std::mem::size_of::<X87GroupSpan32>(), 6);
}

#[test]
fn group_metadata_is_final_prefix_only_and_absent_without_resident_binding() {
    let memory = memory();
    for binding in [
        None,
        Some(wasm::EmbeddedBinding::Replacement {
            key: 7,
            generation: 1,
        }),
    ] {
        let region = CompiledRegion::new(
            prepared(&memory, 0x1000, 8),
            Vec::new(),
            super::super::CompilationTier32::Baseline,
            binding,
        )
        .unwrap();
        assert_eq!(region.x87_groups.capacity(), 0);
        assert!(region.x87_group(0).is_none());
    }
    for length in [1, 3, 4, 7, 8, 9, 12] {
        let region = CompiledRegion::new(
            prepared(&memory, 0x1000, length),
            Vec::new(),
            super::super::CompilationTier32::Baseline,
            resident(),
        )
        .unwrap();
        let expected =
            super::super::x87_groups(&region.prepared.blocks[0].instructions).collect::<Vec<_>>();
        let wide = super::super::x87_wide_groups(&region.prepared.blocks[0].instructions)
            .collect::<Vec<_>>();
        assert_eq!(region.x87_groups.len(), expected.len() + wide.len());
        for (ordinal, plan) in expected.iter().enumerate() {
            assert_eq!(
                region.x87_group(ordinal as u32),
                plan.descriptors(&region.prepared.blocks[0].instructions)
            );
        }
        assert!(region.x87_group(expected.len() as u32).is_none());
        for (index, plan) in wide.iter().enumerate() {
            let ordinal = (expected.len() + index) as u32;
            let view = region.x87_group32(ordinal).unwrap();
            assert_eq!(view.len(), plan.length);
            assert!(region.x87_group(ordinal).is_none());
        }
        if length < 4 {
            assert_eq!(region.x87_groups.capacity(), 0);
        }
        assert!(
            region
                .x87_groups
                .iter()
                .all(|span| usize::from(span.first) + usize::from(span.length) <= length)
        );
    }
}

#[test]
fn group_metadata_does_not_escape_byte_quota_shortening_or_source_currentness() {
    let mut memory = memory();
    let minimum = prepared(&memory, 0x1000, 1);
    let budget = wasm::emit(&minimum.blocks, resident(), minimum.decode_key).len();
    let region = emit_entry_prepared(
        &memory,
        prepared(&memory, 0x1000, 8),
        CompileLimits {
            wasm_bytes: budget,
            ..CompileLimits::default()
        },
        resident(),
        CodeFetchContext::from_segments(&SegmentContext::default()),
    )
    .unwrap();
    assert!(region.metadata().instructions < 4);
    assert_eq!(region.x87_groups.capacity(), 0);
    assert!(region.x87_group(0).is_none());
    assert!(region.wasm_bytes(&memory).unwrap().len() <= budget);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert_eq!(
        region.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
}
