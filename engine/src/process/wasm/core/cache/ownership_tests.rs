use super::*;
use crate::{
    cpu::core::{compile, compile_with_frontier_query},
    memory::{PageRange, Permissions},
};

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

fn range(pc: u32) -> PageRange {
    PageRange::new(GuestAddress(pc), 1).unwrap()
}

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
    memory
        .map_zeroed(range(0x3000), Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(
            GuestAddress(0x1000),
            &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xc3],
        )
        .unwrap();
    memory
}

fn code(memory: &AddressSpace, pc: u32) -> CompiledCode {
    compile(memory, GuestAddress(pc), options()).unwrap()
}

fn ready(result: OwnedEntryLookup) -> RegionRef {
    match result {
        OwnedEntryLookup::Ready(owner) => owner,
        OwnedEntryLookup::Unowned => panic!("expected a retained instruction owner"),
        OwnedEntryLookup::Declined => panic!("entry contract unexpectedly declined"),
    }
}

fn coverage(cache: &mut CoreCache32, memory: &AddressSpace, owner: &RegionRef) {
    for pc in 0x1000..0x1009 {
        assert!(cache.coverage_contains(memory, pc));
        assert_eq!(
            cache.coverage_owner(memory, pc).unwrap().identity(),
            owner.identity()
        );
    }
    assert!(!cache.coverage_contains(memory, 0x1009));
    assert_eq!(cache.coverage.len(), 9);
}

#[test]
fn covered_interior_reforms_one_owner_and_keeps_all_prior_entry_contracts() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let original = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
    cache.install_link(&original, 7).unwrap();
    assert_eq!(original.instruction_count(), 9);
    assert_eq!(original.entrypoints(), &[GuestAddress(0x1000)]);
    assert!(
        !cache.frontier_contains(&memory, 0x1004),
        "covered is not yet executable"
    );
    assert!(cache.lookup(&memory, 0x1004).is_none());
    coverage(&mut cache, &memory, &original);

    let upgraded = ready(
        cache
            .prepare_owned_entry(&memory, 0x1004, options(), &[])
            .unwrap(),
    );
    assert_ne!(upgraded.identity(), original.identity());
    assert!(cache.retained(&original).is_none());
    assert_eq!(
        cache.link_owners[7], 0,
        "the replaced owner's old slot is invalidated"
    );
    assert_eq!(cache.slot_regions[7], NO_REGION);
    assert_eq!(
        upgraded.instruction_pcs().collect::<Vec<_>>(),
        (0x1000..0x1009).collect::<Vec<_>>()
    );
    assert_eq!(
        upgraded.entrypoints(),
        &[GuestAddress(0x1000), GuestAddress(0x1004)]
    );
    assert_eq!(
        upgraded.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1004)]
    );
    assert_eq!(cache.snapshot()[5], 1);
    assert_eq!(cache.bytes, upgraded.wasm().len());
    for pc in [0x1000, 0x1004] {
        assert_eq!(
            cache.lookup(&memory, pc).unwrap().identity(),
            upgraded.identity()
        );
        assert!(cache.frontier_contains(&memory, pc));
    }
    coverage(&mut cache, &memory, &upgraded);

    let before = cache.snapshot();
    let repeated = ready(
        cache
            .prepare_owned_entry(&memory, 0x1004, options(), &[])
            .unwrap(),
    );
    assert_eq!(repeated.identity(), upgraded.identity());
    assert_eq!(
        cache.snapshot(),
        before,
        "an advertised entry does not recompile"
    );
}

#[test]
fn owned_entry_limit_declines_preserve_owner_entries_links_bytes_and_memory() {
    for limited in [
        CompileOptions {
            instructions: 8,
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
        let memory = memory();
        let mut cache = CoreCache32::default();
        let original = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
        cache.install_link(&original, 3).unwrap();
        let before = cache.snapshot();
        let entries = cache.entries.clone();
        let covered = cache.coverage.clone();
        let version = memory.transaction_version();
        for _ in 0..2 {
            assert!(matches!(
                cache
                    .prepare_owned_entry(&memory, 0x1004, limited, &[])
                    .unwrap(),
                OwnedEntryLookup::Declined
            ));
            assert_eq!(cache.snapshot(), before);
            assert_eq!(cache.entries, entries);
            assert_eq!(cache.coverage, covered);
            assert_eq!(cache.link_owners[3], original.identity());
            assert_eq!(
                cache.lookup(&memory, 0x1000).unwrap().identity(),
                original.identity()
            );
            assert!(cache.lookup(&memory, 0x1004).is_none());
            assert_eq!(memory.transaction_version(), version);
        }
        assert_eq!(cache.retained(&original).unwrap().rejected_entries.len(), 1);
        let upgraded = ready(
            cache
                .prepare_owned_entry(&memory, 0x1004, options(), &[])
                .unwrap(),
        );
        assert_ne!(
            upgraded.identity(),
            original.identity(),
            "changed limits retry the exact contract"
        );
        coverage(&mut cache, &memory, &upgraded);
    }
}

#[test]
fn current_instruction_overlap_rejection_is_atomic_but_distinct_decode_starts_are_allowed() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let original = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
    cache.install_link(&original, 1).unwrap();
    let before = cache.snapshot();
    let entries = cache.entries.clone();
    let covered = cache.coverage.clone();
    assert!(matches!(
        cache.admit(&memory, code(&memory, 0x1004)),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.entries, entries);
    assert_eq!(cache.coverage, covered);
    assert_eq!(cache.link_owners[1], original.identity());

    // MOV EAX,0xc3909090; RET has starts 1000/1005. Entering its immediate
    // instead decodes NOP; NOP; NOP; RET at 1001..1004: distinct x86 starts.
    memory
        .write(GuestAddress(0x1000), &[0xb8, 0x90, 0x90, 0x90, 0xc3, 0xc3])
        .unwrap();
    let first = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
    let second = cache.admit(&memory, code(&memory, 0x1001)).unwrap();
    assert_ne!(first.identity(), second.identity());
    assert_eq!(cache.snapshot()[5], 2);
    assert_eq!(
        cache.coverage_owner(&memory, 0x1000).unwrap().identity(),
        first.identity()
    );
    assert_eq!(
        cache.coverage_owner(&memory, 0x1001).unwrap().identity(),
        second.identity()
    );
}

#[test]
fn formation_frontier_cuts_at_owned_interior_without_advertising_it_to_cold_execution() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1040), &[0xeb, 0xc2]).unwrap(); // JMP 1004
    let mut cache = CoreCache32::default();
    let original = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
    let incoming =
        compile_with_frontier_query(&memory, GuestAddress(0x1040), options(), &[], &|pc| {
            cache.coverage_contains(&memory, pc)
        })
        .unwrap();
    assert_eq!(incoming.instruction_pcs().collect::<Vec<_>>(), [0x1040]);
    let incoming = cache.admit(&memory, incoming).unwrap();
    assert_eq!(cache.snapshot()[5], 2);
    assert_eq!(
        cache.coverage_owner(&memory, 0x1004).unwrap().identity(),
        original.identity()
    );
    assert!(!cache.frontier_contains(&memory, 0x1004));
    assert!(cache.frontier_contains(&memory, 0x1040));
    assert_ne!(incoming.identity(), original.identity());
}

#[test]
fn ownership_rejects_source_writes_identical_remaps_and_wrong_address_spaces() {
    for change in ["write", "remap", "space"] {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let original = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
        cache.install_link(&original, 5).unwrap();
        match change {
            "write" => memory.write(GuestAddress(0x1004), &[0x40]).unwrap(),
            "remap" => {
                memory.unmap(range(0x1000)).unwrap();
                memory.map_zeroed(range(0x1000), Permissions::ALL).unwrap();
                memory
                    .write(
                        GuestAddress(0x1000),
                        &[0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xc3],
                    )
                    .unwrap();
            }
            "space" => memory = self::memory(),
            _ => unreachable!(),
        }
        assert!(!cache.coverage_contains(&memory, 0x1004));
        assert!(matches!(
            cache
                .prepare_owned_entry(&memory, 0x1004, options(), &[])
                .unwrap(),
            OwnedEntryLookup::Unowned
        ));
        assert_eq!(cache.snapshot()[5], 0);
        assert_eq!(cache.snapshot()[4], 1);
        assert!(cache.coverage.is_empty() && cache.entries.is_empty());
        assert_eq!(cache.link_owners[5], 0);
        assert_eq!(cache.slot_regions[5], NO_REGION);
    }
}

#[test]
fn a_write_through_nonexecutable_alias_invalidates_interior_coverage_and_its_link() {
    let mut memory = AddressSpace::new(1).unwrap();
    let pages = memory
        .allocate_shared_pages(1, &[0x90, 0x90, 0xc3])
        .unwrap();
    memory
        .map_shared_pages(range(0x1000), &pages, 0, Permissions::READ_EXECUTE, false)
        .unwrap();
    memory
        .map_shared_pages(range(0x3000), &pages, 0, Permissions::READ_WRITE, false)
        .unwrap();
    let mut cache = CoreCache32::default();
    let original = cache.admit(&memory, code(&memory, 0x1000)).unwrap();
    cache.install_link(&original, 2).unwrap();
    assert!(cache.coverage_contains(&memory, 0x1001));
    memory.write(GuestAddress(0x3001), &[0x40]).unwrap();
    assert!(cache.coverage_owner(&memory, 0x1001).is_none());
    assert!(!cache.frontier_contains(&memory, 0x1000));
    assert_eq!(cache.snapshot()[5], 0);
    assert!(cache.entries.is_empty() && cache.coverage.is_empty());
    assert_eq!(cache.link_owners[2], 0);
}

#[test]
fn legacy_overlapping_owners_promote_to_exact_union_and_retire_both_links() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    // Only low-level insert permits this legacy shape; production admit rejects it.
    let first = cache.insert(code(&memory, 0x1000));
    let second = cache.insert(code(&memory, 0x1004));
    assert_eq!(
        (first.instruction_count(), second.instruction_count()),
        (9, 5)
    );
    cache.install_link(&first, 1).unwrap();
    cache.install_link(&second, 2).unwrap();
    assert!(cache.observe_transition(&memory, &first, &second, 0x1004, 1));
    let merged = cache
        .prepare_transition(
            &memory,
            0x1004,
            CompileOptions {
                instructions: 9,
                ..options()
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        merged.instruction_pcs().collect::<Vec<_>>(),
        (0x1000..0x1009).collect::<Vec<_>>()
    );
    assert_eq!(
        merged.entrypoints(),
        &[GuestAddress(0x1000), GuestAddress(0x1004)]
    );
    assert_eq!(
        merged.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1004)]
    );
    assert_eq!(&cache.link_owners[1..3], &[0, 0]);
    assert!(cache.retained(&first).is_none() && cache.retained(&second).is_none());
    assert_eq!(cache.snapshot()[5], 1);
    for pc in [0x1000, 0x1004] {
        assert_eq!(
            cache.lookup(&memory, pc).unwrap().identity(),
            merged.identity()
        );
    }
    coverage(&mut cache, &memory, &merged);
}
