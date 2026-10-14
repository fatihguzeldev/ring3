use super::*;
use crate::{
    cpu::{
        dbt::{
            artifact::{emit_entry_prepared, emit_prepared},
            wasm::EmbeddedBinding,
        },
        x86::{SegmentCache, SegmentContext},
    },
    memory::{PageRange, Permissions},
};
fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 3).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}
fn cs() -> CodeFetchContext {
    CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()
}
#[test]
fn instruction_quota_never_fetches_the_next_instruction_and_guards_only_retained_pages() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1fc0), &[0x90; 64]).unwrap();
    let prepared = prepare_runtime_entry(
        &memory,
        GuestAddress(0x1fc0),
        CompileLimits::default(),
        Some(cs()),
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
    .unwrap();
    assert_eq!(prepared.instruction_count(), 64);
    assert_eq!(
        prepared.blocks[0].instructions.last().unwrap().next_pc(),
        GuestAddress(0x2000)
    );
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0xf0, 0x90]).unwrap();
    assert!(prepared.is_current(&memory));
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    assert!(!prepared.is_current(&memory));
}
#[test]
fn discovery_preserves_every_requested_entry_under_the_instruction_budget() {
    let mut memory = memory();
    for pc in [0x1100, 0x2100] {
        memory.write(GuestAddress(pc), &[0x90; 80]).unwrap();
    }
    let entries = [GuestAddress(0x1100), GuestAddress(0x2100)];
    let prepared = prepare_runtime_entries_for_test(&memory, &entries, CompileLimits::default());
    assert_eq!(prepared.instruction_count(), 64);
    assert_eq!(prepared.blocks.len(), 2);
    assert_eq!(prepared.blocks[0].instructions.len(), 63);
    assert_eq!(prepared.blocks[1].instructions.len(), 1);
    assert_eq!(prepared.blocks[1].instructions[0].pc(), entries[1]);
    assert!(matches!(
        prepare_embedded_entry_region(
            &memory,
            &entries,
            CompileLimits {
                instructions: 1,
                ..CompileLimits::default()
            },
            &[],
            Some(cs()),
            crate::cpu::x86::decode::DecodeAuthority::default()
        ),
        Err(CompileError::InstructionLimit)
    ));
}
fn prepare_runtime_entries_for_test(
    memory: &AddressSpace,
    entries: &[GuestAddress],
    limits: CompileLimits,
) -> PreparedRegion {
    prepare_embedded_entry_region(
        memory,
        entries,
        limits,
        &[],
        Some(cs()),
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
    .unwrap()
}
#[test]
fn byte_retry_rebuilds_the_snapshot_and_keeps_all_entries() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1fff), &[0x90; 30]).unwrap();
    memory.write(GuestAddress(0x201d), &[0xeb, 0]).unwrap();
    memory
        .write(GuestAddress(0x3100), &[0x90, 0xeb, 0])
        .unwrap();
    let entries = [GuestAddress(0x1fff), GuestAddress(0x3100)];
    let binding = Some(EmbeddedBinding::Replacement {
        key: 17,
        generation: 1,
    });
    let minimum = prepare_runtime_entries_for_test(
        &memory,
        &entries,
        CompileLimits {
            instructions: 2,
            ..CompileLimits::default()
        },
    );
    let minimum_bytes = crate::cpu::dbt::wasm::emit(&minimum.blocks, binding, minimum.decode_key);
    let limits = CompileLimits {
        wasm_bytes: minimum_bytes.len(),
        ..CompileLimits::default()
    };
    let full = prepare_runtime_entries_for_test(&memory, &entries, limits);
    assert!(matches!(
        emit_prepared(full, limits, binding),
        Err(CompileError::WasmLimit)
    ));
    let full = prepare_runtime_entries_for_test(&memory, &entries, limits);
    let artifact = emit_entry_prepared(&memory, full, limits, binding, Some(cs())).unwrap();
    assert_eq!(artifact.metadata().blocks, 2);
    assert_eq!(artifact.metadata().instructions, 2);
    assert_eq!(artifact.wasm_bytes(&memory).unwrap(), minimum_bytes);
    memory.write(GuestAddress(0x2000), &[0xf0]).unwrap();
    assert!(artifact.wasm_bytes(&memory).is_ok());
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    assert!(artifact.wasm_bytes(&memory).is_err());
}
#[test]
fn shrinking_a_context_fault_removes_its_consumed_prefix_pages() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1fff), &[0x90, 0xb8, 0x12])
        .unwrap();
    let mut context = SegmentContext::default();
    context.caches[1] = SegmentCache::from_bits(0xf2b02001).unwrap();
    let cs = CodeFetchContext::from_segments(&context).unwrap();
    let mut prepared = prepare_runtime_entry(
        &memory,
        GuestAddress(0x1fff),
        CompileLimits::default(),
        Some(cs),
        crate::cpu::x86::decode::DecodeAuthority::default(),
    )
    .unwrap();
    assert_eq!(prepared.instruction_count(), 2);
    assert_eq!(
        prepared.blocks[0]
            .context_fault()
            .unwrap()
            .consumed_prefix_length,
        2
    );
    assert!(shorten_entry_region(&memory, &mut prepared, Some(cs)).unwrap());
    assert_eq!(prepared.instruction_count(), 1);
    assert!(prepared.blocks[0].context_fault().is_none());
    assert!(!shorten_entry_region(&memory, &mut prepared, Some(cs)).unwrap());
    memory.write(GuestAddress(0x2000), &[0xb8]).unwrap();
    assert!(prepared.is_current(&memory));
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    assert!(!prepared.is_current(&memory));
}
