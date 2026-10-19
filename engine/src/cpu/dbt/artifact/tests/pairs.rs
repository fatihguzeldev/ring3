use super::*;
use crate::{
    cpu::{
        dbt::region::prepare_resident_region,
        x86::{SegmentContext, decode::DecodeAuthority},
    },
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}

fn prepared(memory: &AddressSpace, specs: &[BlockSpec]) -> PreparedRegion {
    prepare_resident_region(
        memory,
        specs,
        CompileLimits::default(),
        &[],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

fn resident() -> Option<wasm::EmbeddedBinding> {
    Some(wasm::EmbeddedBinding::Resident { key: 7, id: 1 })
}

#[test]
fn pair_lookup_keeps_greedy_entries_sorted_across_unordered_blocks() {
    let mut memory = memory();
    // fld1; fldz; fadd st0,st1; fchs; fabs; jmp edx.
    let code = [
        0xd9, 0xe8, 0xd9, 0xee, 0xd8, 0xc1, 0xd9, 0xe0, 0xd9, 0xe1, 0xff, 0xe2,
    ];
    for pc in [0x1000, 0x2000] {
        memory.write(GuestAddress(pc), &code).unwrap();
    }
    let region = emit_prepared(
        prepared(
            &memory,
            &[0x2000, 0x1000].map(|pc| BlockSpec {
                entry: GuestAddress(pc),
                byte_length: code.len() as u32,
            }),
        ),
        CompileLimits::default(),
        resident(),
    )
    .unwrap();
    assert_eq!(
        region
            .x87_pairs
            .iter()
            .map(|pair| pair[0].pc)
            .collect::<Vec<_>>(),
        [0x1000, 0x1004, 0x2000, 0x2004]
    );
    let expected = region
        .prepared
        .blocks
        .iter()
        .flat_map(|block| super::super::x87_pairs(&block.instructions))
        .collect::<Vec<_>>();
    for pair in expected {
        assert_eq!(region.x87_pair(pair[0].pc), Some(pair));
    }
    for pc in [0, 0x1001, 0x1002, 0x1006, 0x1008, 0x1800, 0x2002, u32::MAX] {
        assert!(
            region.x87_pair(pc).is_none(),
            "not a selected first entry: {pc:x}"
        );
    }
}

#[test]
fn pair_metadata_is_absent_without_resident_binding_or_eligible_pairs() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xd9, 0xe8, 0xd9, 0xee, 0xff, 0xe2])
        .unwrap();
    let specs = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: 6,
    }];
    let replacement = emit_prepared(
        prepared(&memory, &specs),
        CompileLimits::default(),
        Some(wasm::EmbeddedBinding::Replacement {
            key: 7,
            generation: 1,
        }),
    )
    .unwrap();
    assert_eq!(replacement.x87_pairs.capacity(), 0);
    assert!(replacement.x87_pair(0x1000).is_none());

    memory
        .write(GuestAddress(0x1000), &[0x90, 0x90, 0xff, 0xe2])
        .unwrap();
    let specs = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: 4,
    }];
    for binding in [None, resident()] {
        let region =
            emit_prepared(prepared(&memory, &specs), CompileLimits::default(), binding).unwrap();
        assert_eq!(region.x87_pairs.capacity(), 0);
        assert!(region.x87_pair(0x1000).is_none());
    }
}

#[test]
fn pair_metadata_is_built_from_only_the_final_emitted_prefix() {
    let mut memory = memory();
    let code = [
        0xd9, 0xe8, 0xd9, 0xee, 0xd8, 0xc1, 0xd9, 0xe0, 0xd9, 0xe1, 0xd9, 0xe0, 0xff, 0xe2,
    ];
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let specs = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: code.len() as u32,
    }];
    let mut minimum = prepared(&memory, &specs);
    minimum.blocks[0].instructions.truncate(1);
    let budget = wasm::emit(&minimum.blocks, resident(), minimum.decode_key).len();
    let region = emit_entry_prepared(
        &memory,
        prepared(&memory, &specs),
        CompileLimits {
            wasm_bytes: budget,
            ..CompileLimits::default()
        },
        resident(),
        CodeFetchContext::from_segments(&SegmentContext::default()),
    )
    .unwrap();
    assert_eq!(region.metadata().instructions, 1);
    assert_eq!(region.x87_pairs.capacity(), 0);
    assert!(region.x87_pair(0x1000).is_none());
    assert!(region.x87_pair(0x1004).is_none());
    assert!(region.wasm_bytes(&memory).unwrap().len() <= budget);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert_eq!(
        region.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
}
