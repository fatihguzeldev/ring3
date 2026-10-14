use super::*;
use crate::{
    cpu::x86::{
        SegmentContext,
        decode::{CapturedFetchFaultKind32, DecodeAuthority},
    },
    memory::{PageRange, Permissions},
};

#[test]
fn automatic_paging_terminal_retains_only_consumed_source_and_repair_rechecks_authority() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1ffe), &[0x90, 0xb8]).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let prepared = prepare_runtime_entry(
        &memory,
        GuestAddress(0x1ffe),
        CompileLimits::default(),
        Some(cs),
        DecodeAuthority::default(),
    )
    .unwrap();
    assert_eq!(prepared.instruction_count(), 2);
    assert_eq!(prepared.blocks[0].instructions.len(), 1);
    let fault = prepared.blocks[0].context_fault().unwrap();
    assert_eq!(fault.kind, CapturedFetchFaultKind32::Paging);
    assert_eq!(
        (
            fault.pc,
            fault.consumed_prefix_length,
            fault.attempted_length
        ),
        (GuestAddress(0x1fff), 1, 2)
    );
    assert!(fault.prefix.is_none());
    assert!(matches!(
        &prepared.blocks[0].source,
        BlockSource::ContextFault {
            consumed: Some(_),
            ..
        }
    ));
    assert!(prepared.is_current(&memory));
    let source_free = prepare_runtime_entry(
        &memory,
        GuestAddress(0x2000),
        CompileLimits::default(),
        Some(cs),
        DecodeAuthority::default(),
    )
    .unwrap();
    assert_eq!(source_free.instruction_count(), 1);
    assert!(source_free.blocks[0].instructions.is_empty());
    assert!(matches!(
        &source_free.blocks[0].source,
        BlockSource::ContextFault { consumed: None, .. }
    ));
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x2000), &[0, 0, 0, 0, 0xeb, 0xfe])
        .unwrap();
    assert!(!fault.remains_denied(&memory, cs));
    assert!(prepared.is_current(&memory));
    assert!(source_free.is_current(&memory));
    assert!(
        !source_free.blocks[0]
            .context_fault()
            .unwrap()
            .remains_denied(&memory, cs)
    );
    memory.write(GuestAddress(0x1fff), &[0xb9]).unwrap();
    assert!(!prepared.is_current(&memory));
}
