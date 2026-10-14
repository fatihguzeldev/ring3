use super::super::super::{EmbeddedBinding, emitter::emit_with_authority};
use crate::{
    cpu::{
        dbt::{CompileLimits, cold::prepare_runtime_entry},
        x86::{
            SegmentCache, SegmentContext, State32,
            decode::{CodeFetchContext, DecodeAuthority},
            tasks::{TaskContextImage32, TaskIncomingContext32},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn raw() -> TaskIncomingContext32 {
    let mut segments = SegmentContext::default();
    segments.state.selectors[3] = 0x4b;
    segments.caches[3] = SegmentCache::UNUSABLE;
    TaskIncomingContext32::from_image(TaskContextImage32 {
        state: State32::default(),
        segments,
        loaded: 0x37,
        ldtr: 0,
        ldt_cache: None,
        task_trap: false,
        fault: None,
    })
    .unwrap()
}
#[test]
fn raw_task_presence_is_separate_from_code_interpretation_and_strict_full_context() {
    let partial = raw();
    assert!(partial.validated_segments().is_none());
    let cs = CodeFetchContext::from_task_context(&partial).unwrap();
    assert!(cs.raw_task_context());
    let normal = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    assert!(!normal.raw_task_context());
    assert_eq!(cs.decode_key(), normal.decode_key());
    assert!(CodeFetchContext::from_segments(&partial.segment_image()).is_none());
}
#[test]
fn only_partial_bound_modules_have_refresh_import_and_revision_globals() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &[0xeb, 0xfe]).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let prepared = prepare_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        CompileLimits::default(),
        Some(cs),
        DecodeAuthority::default(),
    )
    .unwrap();
    let binding = Some(EmbeddedBinding::Replacement {
        key: 7,
        generation: 1,
    });
    let full = emit_with_authority(
        &prepared.blocks,
        binding,
        Some(cs.decode_key()),
        false,
        false,
        false,
        None,
    );
    let partial = emit_with_authority(
        &prepared.blocks,
        binding,
        Some(cs.decode_key()),
        false,
        false,
        false,
        Some(17),
    );
    let name = b"read_task_context";
    assert!(!full.windows(name.len()).any(|bytes| bytes == name));
    assert!(partial.windows(name.len()).any(|bytes| bytes == name));
    assert!(partial.len() > full.len());
    // differing task identity is retained in module data independently of identical code bytes/key.
    let other = emit_with_authority(
        &prepared.blocks,
        binding,
        Some(cs.decode_key()),
        false,
        false,
        false,
        Some(18),
    );
    assert_ne!(partial, other);
    let zero = emit_with_authority(
        &prepared.blocks,
        binding,
        Some(cs.decode_key()),
        false,
        false,
        false,
        Some(0),
    );
    assert_ne!(partial, zero);
}
