use super::super::{
    ArtifactError, compile_embedded_entry_region, compile_embedded_region, compile_entry_region,
};
use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn embedded_seeds_keep_order_and_match_explicit_gate_memory_stack_emission() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for (address, bytes) in [
        (0x1100, &[0x0f, 0x0b][..]),
        (0x1300, &[0x0f, 0x0b][..]),
        (0x1400, &[0x8b, 0x44, 0x24, 4, 0x50, 0xff, 0x17, 0xf4][..]),
        (0x1600, &[0xc2, 8, 0][..]),
    ] {
        memory.write(GuestAddress(address), bytes).unwrap();
    }
    let entries = [0x1100, 0x1400, 0x1600, 0x1300].map(GuestAddress);
    let gates = [
        GateSpec {
            entry: entries[0],
            id: 17,
        },
        GateSpec {
            entry: entries[3],
            id: 18,
        },
    ];
    let specs =
        [(0x1100, 2), (0x1400, 7), (0x1600, 3), (0x1300, 2)].map(|(pc, length)| BlockSpec {
            entry: GuestAddress(pc),
            byte_length: length,
        });
    let limits = CompileLimits::default();
    let prepared = prepare_embedded_entry_region(&memory, &entries, limits, &gates).unwrap();
    assert_eq!(
        (prepared.block_count(), prepared.instruction_count()),
        (4, 6)
    );
    assert_eq!(prepared.blocks[0].gate.as_ref().unwrap().entry, entries[0]);
    assert_eq!(
        prepared.blocks[1]
            .instructions
            .iter()
            .map(|instruction| instruction.pc().0)
            .collect::<Vec<_>>(),
        [0x1400, 0x1404, 0x1405]
    );
    assert_eq!(prepared.blocks[2].instructions[0].pc(), entries[2]);
    assert_eq!(prepared.blocks[3].gate.as_ref().unwrap().entry, entries[3]);
    assert!(prepared.is_current(&memory));
    // no stack or indirect-source data page exists; preparation uses Execute bytes only.
    let discovered =
        compile_embedded_entry_region(&memory, &entries, limits, 91, 7, &gates).unwrap();
    let explicit = compile_embedded_region(&memory, &specs, limits, 91, 7, &gates).unwrap();
    assert_eq!(discovered.metadata(), explicit.metadata());
    assert_eq!(
        discovered.wasm_bytes(&memory).unwrap(),
        explicit.wasm_bytes(&memory).unwrap()
    );
}

#[test]
fn terminal_snapshot_omits_unfetched_successor_page_and_still_guards_consumed_bytes() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1ffe), &[0xeb, 0]).unwrap();
    memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
    let artifact =
        compile_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default()).unwrap();
    assert_eq!(artifact.metadata().instructions, 1);
    memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
    assert!(artifact.wasm_bytes(&memory).is_ok());
    memory.write(GuestAddress(0x1ffe), &[0xeb, 0]).unwrap();
    assert_eq!(
        artifact.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
}
