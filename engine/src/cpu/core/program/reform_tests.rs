use super::*;
use crate::memory::{PageRange, Permissions};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0x1000, 0x2000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
}

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

#[test]
fn a_new_interior_entry_keeps_exact_coverage_and_all_prior_entries_and_roots() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x41, 0x42, 0x0f, 0x0b])
        .unwrap();
    let original = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    let first = original
        .with_entry(&memory, GuestAddress(0x1002), options(), &[])
        .unwrap()
        .unwrap();
    let second = first
        .with_entry(&memory, GuestAddress(0x1001), options(), &[])
        .unwrap()
        .unwrap();
    assert_ne!(original.identity(), first.identity());
    assert_ne!(first.identity(), second.identity());
    assert_eq!(second.instruction_pcs, original.instruction_pcs);
    for code in [&first, &second] {
        assert!(code.is_current(&memory));
        assert!(code.entrypoints().contains(&GuestAddress(0x1000)));
    }
    assert_eq!(
        second.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1001),
            GuestAddress(0x1002)
        ]
    );
    assert_eq!(second.formation_roots(), second.entrypoints());
    assert_eq!(original.entrypoints(), &[GuestAddress(0x1000)]);
}

#[test]
fn reform_does_not_follow_a_previously_excluded_successor_or_instruction_interior() {
    let mut memory = memory();
    memory
        .write(
            GuestAddress(0x1000),
            &[0xb8, 1, 0, 0, 0, 0x40, 0xe9, 0xf5, 0x0f, 0, 0],
        )
        .unwrap();
    memory.write(GuestAddress(0x2000), &[0xc3]).unwrap();
    let original = compile_with_frontier(
        &memory,
        GuestAddress(0x1000),
        options(),
        &[],
        &[GuestAddress(0x2000)],
    )
    .unwrap();
    assert!(
        original
            .with_entry(&memory, GuestAddress(0x1001), options(), &[])
            .unwrap()
            .is_none()
    );
    let reformed = original
        .with_entry(&memory, GuestAddress(0x1005), options(), &[])
        .unwrap()
        .unwrap();
    assert_eq!(reformed.instruction_pcs, original.instruction_pcs);
    assert!(!reformed.instruction_pcs.contains(&0x2000));
}

#[test]
fn exact_reform_declines_each_budget_without_shortening_or_mutating_the_owner() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x41, 0x42, 0x0f, 0x0b])
        .unwrap();
    let original = compile(&memory, GuestAddress(0x1000), options()).unwrap();
    let identity = original.identity();
    let bytes = original.wasm().to_vec();
    for limits in [
        CompileOptions {
            instructions: 3,
            ..options()
        },
        CompileOptions {
            blocks: 1,
            ..options()
        },
        CompileOptions {
            wasm_bytes: 128,
            ..options()
        },
    ] {
        assert!(
            original
                .with_entry(&memory, GuestAddress(0x1001), limits, &[])
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(original.identity(), identity);
    assert_eq!(original.wasm(), bytes);
    assert_eq!(original.entrypoints(), &[GuestAddress(0x1000)]);
    assert_eq!(
        original.instruction_pcs.as_ref(),
        &[0x1000, 0x1001, 0x1002, 0x1003]
    );
    assert!(original.is_current(&memory));
}

#[test]
fn source_or_address_space_changes_decline_reform() {
    let mut first = memory();
    first
        .write(GuestAddress(0x1000), &[0x40, 0x41, 0x0f, 0x0b])
        .unwrap();
    let original = compile(&first, GuestAddress(0x1000), options()).unwrap();
    let mut second = memory();
    second
        .write(GuestAddress(0x1000), &[0x40, 0x41, 0x0f, 0x0b])
        .unwrap();
    assert!(
        original
            .with_entry(&second, GuestAddress(0x1001), options(), &[])
            .unwrap()
            .is_none()
    );
    first.write(GuestAddress(0x1000), &[0x42]).unwrap();
    assert!(
        original
            .with_entry(&first, GuestAddress(0x1001), options(), &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn union_reform_preserves_both_exact_owners_and_gate_instruction_starts() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x0f, 0x0b])
        .unwrap();
    memory
        .write(GuestAddress(0x2000), &[0x41, 0x0f, 0x0b])
        .unwrap();
    let gates = [(GuestAddress(0x1001), 7), (GuestAddress(0x2001), 9)];
    let first = compile_with_gates(&memory, GuestAddress(0x1000), options(), &gates).unwrap();
    let second = compile_with_gates(&memory, GuestAddress(0x2000), options(), &gates).unwrap();
    let union = CompiledCode::reform_union(
        &memory,
        &[&first, &second],
        GuestAddress(0x2001),
        options(),
        &gates,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        union.instruction_pcs.as_ref(),
        &[0x1000, 0x1001, 0x2000, 0x2001]
    );
    assert_eq!(
        union.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x2000),
            GuestAddress(0x2001)
        ]
    );
    assert_eq!(union.formation_roots(), union.entrypoints());
    assert!(union.native_instructions().is_empty());
    assert!(union.is_current(&memory));
}

#[test]
fn borrowed_frontier_query_has_the_same_entry_exception_as_the_slice_api() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0x41, 0x0f, 0x0b])
        .unwrap();
    let query = |pc| pc == 0x1000 || pc == 0x1001;
    let code =
        compile_with_frontier_query(&memory, GuestAddress(0x1000), options(), &[], &query).unwrap();
    assert_eq!(code.instruction_pcs.as_ref(), &[0x1000]);
    assert_eq!(code.entrypoints(), &[GuestAddress(0x1000)]);
}
