use super::*;
use crate::{
    cpu::{core::compile, x86::State32},
    memory::{PageRange, Permissions},
    process::{EngineInstance, core_numeric_tests::NumericBank},
};

fn options() -> CompileOptions {
    CompileOptions {
        instructions: 64,
        blocks: 8,
        ..CompileOptions::default()
    }
}

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for (pc, permissions) in [
        (0x1000, Permissions::ALL),
        (0x2000, Permissions::ALL),
        (0x3000, Permissions::READ_WRITE),
    ] {
        memory
            .map_zeroed(PageRange::new(GuestAddress(pc), 1).unwrap(), permissions)
            .unwrap();
    }
    memory
        .write(
            GuestAddress(0x1000),
            &[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1040), &[0xe8, 0x3b, 0, 0, 0, 0xc3])
        .unwrap();
    memory.write(GuestAddress(0x1080), &[0x40, 0xc3]).unwrap();
    memory.write(GuestAddress(0x2000), &[0x40, 0xc3]).unwrap();
    memory
}

fn insert(cache: &mut CoreCache32, memory: &AddressSpace, pc: u32, slot: u32) -> RegionRef {
    let code = cache.insert(compile(memory, GuestAddress(pc), options()).unwrap());
    cache.install_link(&code, slot).unwrap();
    code
}

fn mature(
    cache: &mut CoreCache32,
    memory: &AddressSpace,
    source: &RegionRef,
    target: &RegionRef,
    pc: u32,
) {
    assert!(!cache.observe_transition(memory, source, target, pc, 2));
    assert!(cache.observe_transition(memory, source, target, pc, 2));
}

fn ineligible_pair() -> (AddressSpace, CoreCache32, RegionRef, RegionRef) {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = insert(&mut cache, &memory, 0x1000, 1);
    let code = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x2000),
        CompileOptions {
            instructions: 3,
            ..options()
        },
        &[],
        &[],
        &[GuestAddress(0x1000), GuestAddress(0x2000)],
    )
    .unwrap()
    .unwrap();
    let target = cache.insert(code);
    cache.install_link(&target, 2).unwrap();
    // the target contains the source root, but its three-instruction prefix
    // leaves the source's caller continuation independently indexed.
    assert!(cache.indexes(&source, 0x1005));
    assert!(cache.indexes(&target, 0x1000));
    assert!(cache.indexes(&target, 0x2000));
    assert!(!formation_candidate(&source, &target, 0x2000));
    (memory, cache, source, target)
}

fn shadowed_root_pair() -> (AddressSpace, CoreCache32, RegionRef, RegionRef, RegionRef) {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xe9, 0xfb, 0x0f, 0, 0])
        .unwrap();
    memory.write(GuestAddress(0x1040), &[0xc3]).unwrap();
    let code = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1080),
        options(),
        &[],
        &[GuestAddress(0x2000)],
        &[GuestAddress(0x1000), GuestAddress(0x1080)],
    )
    .unwrap()
    .unwrap();
    let mut cache = CoreCache32::default();
    let source = cache.insert(code);
    cache.install_link(&source, 1).unwrap();
    let target = insert(&mut cache, &memory, 0x1040, 2);
    let third = insert(&mut cache, &memory, 0x1000, 3);
    assert!(cache.indexes(&source, 0x1080));
    assert!(cache.indexes(&target, 0x1040));
    assert!(cache.indexes(&third, 0x1000));
    assert_eq!(cache.retained(&source).unwrap().indexed_entries, 1);
    assert_eq!(cache.retained(&target).unwrap().indexed_entries, 1);
    (memory, cache, source, target, third)
}

#[test]
fn a_root_limit_declines_before_collecting_unrelated_frontiers() {
    let (mut memory, mut cache, source, target, third) = shadowed_root_pair();
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(source.is_current(&memory) && target.is_current(&memory));
    assert!(!third.is_current(&memory));
    assert!(
        cache
            .transition_roots(&memory, &source, &target, 0x1040, 2)
            .is_none()
    );
    assert_eq!(
        cache.transition_roots(&memory, &source, &target, 0x1040, 3),
        Some(vec![
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1080)
        ])
    );
    mature(&mut cache, &memory, &source, &target, 0x1040);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(
                &memory,
                0x1040,
                CompileOptions {
                    blocks: 2,
                    ..options()
                },
                &[]
            )
            .is_none()
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        &cache.link_owners[1..4],
        &[source.identity(), target.identity(), third.identity()]
    );
}

#[test]
fn a_current_third_owner_root_declines_without_changing_owners() {
    let (memory, mut cache, source, target, third) = shadowed_root_pair();
    assert!(
        cache
            .transition_roots(&memory, &source, &target, 0x1040, 8)
            .is_none()
    );
    mature(&mut cache, &memory, &source, &target, 0x1040);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        &cache.link_owners[1..4],
        &[source.identity(), target.identity(), third.identity()]
    );
    assert_eq!(
        cache.lookup(&memory, 0x1080).unwrap().identity(),
        source.identity()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1040).unwrap().identity(),
        target.identity()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1000).unwrap().identity(),
        third.identity()
    );
}

#[test]
fn a_stale_third_owner_root_does_not_block_a_current_formation() {
    let (mut memory, mut cache, source, target, third) = shadowed_root_pair();
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(source.is_current(&memory) && target.is_current(&memory));
    assert!(!third.is_current(&memory));
    assert_eq!(
        cache.transition_roots(&memory, &source, &target, 0x1040, 8),
        Some(vec![
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1080)
        ])
    );
    mature(&mut cache, &memory, &source, &target, 0x1040);
    let merged = cache
        .prepare_transition(&memory, 0x1040, options(), &[])
        .unwrap();
    assert_eq!(
        merged.formation_roots(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1080)
        ]
    );
    assert!(merged.is_current(&memory));
    assert!(cache.retained(&source).is_none() && cache.retained(&target).is_none());
}

#[test]
fn repeated_ineligible_pairs_are_memoized_without_disturbing_positive_heat() {
    let (memory, mut cache, source, target) = ineligible_pair();
    let eligible = insert(&mut cache, &memory, 0x1080, 3);
    assert!(!cache.observe_transition(&memory, &source, &eligible, 0x1080, 3));
    let heat = &cache.retained(&source).unwrap().heat;
    let edges = heat
        .edges
        .map(|edge| (edge.target, edge.target_index, edge.pc, edge.hits));
    let rejected = heat.rejected;
    let snapshot = cache.snapshot();
    let owners = cache.link_owners.to_vec();
    for pc in [0x2000, 0x2000, 0x1000, 0x1000, 0x2000] {
        for _ in 0..8 {
            assert!(!cache.observe_transition(&memory, &source, &target, pc, 1));
            assert_eq!(
                cache.retained(&source).unwrap().heat.ineligible,
                Some(IneligibleTransition {
                    target: target.identity(),
                    pc,
                })
            );
            assert!(!cache.transition_pending());
        }
    }
    let heat = &cache.retained(&source).unwrap().heat;
    assert_eq!(
        heat.edges
            .map(|edge| (edge.target, edge.target_index, edge.pc, edge.hits)),
        edges
    );
    assert!(heat.rejected == rejected);
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners.as_ref(), owners);
    assert!(!cache.observe_transition(&memory, &source, &eligible, 0x1080, 3));
    assert!(cache.observe_transition(&memory, &source, &eligible, 0x1080, 3));
    assert_eq!(
        cache.pending_transition.as_ref().unwrap().target,
        eligible.key
    );
}

#[test]
fn a_reused_target_at_the_same_pc_cannot_inherit_an_ineligible_memo() {
    let (memory, mut cache, source, target) = ineligible_pair();
    assert!(!cache.observe_transition(&memory, &source, &target, 0x2000, 1));
    assert!(cache.retained(&source).unwrap().heat.ineligible.is_some());
    cache.remove(target.key);
    let replacement = insert(&mut cache, &memory, 0x2000, 2);
    assert_eq!(replacement.key.index, target.key.index);
    assert_ne!(replacement.identity(), target.identity());
    assert!(target.is_current(&memory));
    assert!(formation_candidate(&source, &replacement, 0x2000));
    assert!(!cache.observe_transition(&memory, &source, &target, 0x2000, 1));
    assert!(!cache.observe_transition(&memory, &source, &replacement, 0x2000, 2));
    assert!(cache.observe_transition(&memory, &source, &replacement, 0x2000, 2));
    assert_eq!(
        cache.pending_transition.as_ref().unwrap().target,
        replacement.key
    );
}

#[test]
fn an_ineligible_memo_cannot_bypass_code_or_source_owner_validation() {
    for change_target in [false, true] {
        let (mut memory, mut cache, source, target) = ineligible_pair();
        assert!(!cache.observe_transition(&memory, &source, &target, 0x2000, 1));
        assert!(cache.retained(&source).unwrap().heat.ineligible.is_some());
        if change_target {
            memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
            assert!(source.is_current(&memory));
            assert!(!target.is_current(&memory));
            assert!(matches!(
                cache.lookup_transition(&memory, &source, 0x2000, 1),
                TransitionLookup::Miss
            ));
            assert!(cache.retained(&target).is_none());
            assert_eq!(cache.link_owners[2], 0);
            assert_eq!(cache.source_invalidations, 1);
        } else {
            cache.remove(source.key);
            let replacement = insert(&mut cache, &memory, 0x1000, 1);
            assert_eq!(replacement.key.index, source.key.index);
            assert_ne!(replacement.identity(), source.identity());
            assert!(source.is_current(&memory));
            assert!(!cache.observe_transition(&memory, &source, &target, 0x2000, 1));
            assert!(
                cache
                    .retained(&replacement)
                    .unwrap()
                    .heat
                    .ineligible
                    .is_none()
            );
        }
        assert!(!cache.transition_pending());
    }
}

#[test]
fn unchanged_import_retains_a_pending_pair_without_changing_cache_ownership() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let snapshot = cache.snapshot();
    let owners = cache.link_owners.to_vec();
    cache.retain_transition_on_import(0x1040);
    assert!(cache.transition_pending());
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners.as_ref(), owners);
    let formed = cache
        .prepare_transition(&memory, 0x1040, options(), &[])
        .unwrap();
    assert_eq!(
        formed.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1040)]
    );
    assert_eq!(&cache.link_owners[1..3], &[0, 0]);
    assert!(cache.retained(&a).is_none() && cache.retained(&b).is_none());
}

#[test]
fn changed_import_pc_discards_pending_heat_without_reassigning_either_owner() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let snapshot = cache.snapshot();
    cache.retain_transition_on_import(0x1000);
    assert!(!cache.transition_pending());
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
}

#[test]
fn changed_import_memory_or_code_epoch_cannot_form_a_pending_pair() {
    for change in 0..3 {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let a = insert(&mut cache, &memory, 0x1000, 1);
        let b = insert(&mut cache, &memory, 0x1040, 2);
        mature(&mut cache, &memory, &a, &b, 0x1040);
        let other;
        let imported = match change {
            0 => {
                other = self::memory();
                assert_eq!(other.code_source_version(), memory.code_source_version());
                &other
            }
            1 => {
                memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
                &memory
            }
            _ => {
                let mut bytes = [0; 4096];
                memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
                let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
                memory.unmap(range).unwrap();
                memory.map_zeroed(range, Permissions::ALL).unwrap();
                memory.write(GuestAddress(0x1000), &bytes).unwrap();
                &memory
            }
        };
        let snapshot = cache.snapshot();
        cache.retain_transition_on_import(0x1040);
        assert!(cache.transition_pending());
        assert!(
            cache
                .prepare_transition(imported, 0x1040, options(), &[])
                .is_none()
        );
        assert!(!cache.transition_pending());
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
    }
}

#[test]
fn reused_pending_owner_or_driver_slot_cannot_form_after_import() {
    for replace_target in [false, true] {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let a = insert(&mut cache, &memory, 0x1000, 1);
        let b = insert(&mut cache, &memory, 0x1040, 2);
        mature(&mut cache, &memory, &a, &b, 0x1040);
        let removed = if replace_target { &b } else { &a };
        let slot = if replace_target { 2 } else { 1 };
        cache.remove(removed.key);
        let replacement = insert(&mut cache, &memory, 0x2000, slot);
        assert_eq!(replacement.key.index, removed.key.index);
        assert!(removed.is_current(&memory));
        let snapshot = cache.snapshot();
        let owners = cache.link_owners.to_vec();
        cache.retain_transition_on_import(0x1040);
        assert!(cache.transition_pending());
        assert!(
            cache
                .prepare_transition(&memory, 0x1040, options(), &[])
                .is_none()
        );
        assert!(!cache.transition_pending());
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!(cache.link_owners.as_ref(), owners);
    }
}

#[test]
fn unlinked_or_shadowed_pending_target_cannot_form_after_import() {
    for shadow in [false, true] {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let a = insert(&mut cache, &memory, 0x1000, 1);
        let b = insert(&mut cache, &memory, 0x1040, 2);
        mature(&mut cache, &memory, &a, &b, 0x1040);
        if shadow {
            insert(&mut cache, &memory, 0x1040, 3);
        } else {
            cache.unlink(2, b.identity()).unwrap();
        }
        let snapshot = cache.snapshot();
        let owners = cache.link_owners.to_vec();
        cache.retain_transition_on_import(0x1040);
        assert!(cache.transition_pending());
        assert!(
            cache
                .prepare_transition(&memory, 0x1040, options(), &[])
                .is_none()
        );
        assert!(!cache.transition_pending());
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!(cache.link_owners.as_ref(), owners);
    }
}

#[test]
fn a_mature_observed_pair_is_formed_only_by_the_closed_prepare() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    let before = cache.snapshot();
    mature(&mut cache, &memory, &a, &b, 0x1040);
    assert_eq!(cache.snapshot(), before);
    assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
    let merged = cache
        .prepare_transition(&memory, 0x1040, options(), &[])
        .unwrap();
    assert_ne!(merged.identity(), a.identity());
    assert_ne!(merged.identity(), b.identity());
    assert_eq!(
        merged.formation_roots(),
        &[GuestAddress(0x1000), GuestAddress(0x1040)]
    );
    assert!(Rc::ptr_eq(
        &cache.lookup(&memory, 0x1005).unwrap().code,
        &merged.code
    ));
    assert!(Rc::ptr_eq(
        &cache.lookup(&memory, 0x1040).unwrap().code,
        &merged.code
    ));
    assert_eq!(&cache.link_owners[1..3], &[0, 0]);
    assert!(cache.retained(&a).is_none() && cache.retained(&b).is_none());
    assert!(a.is_current(&memory) && b.is_current(&memory));
    assert_eq!(cache.snapshot()[5], 1);
    assert_eq!(cache.snapshot()[6], merged.wasm().len() as u64);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(cache.snapshot()[0], before[0] + 1);
}

#[test]
fn a_return_into_an_interior_entry_keeps_all_previously_formed_roots() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    let c = insert(&mut cache, &memory, 0x1080, 3);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let ab = cache
        .prepare_transition(&memory, 0x1040, options(), &[])
        .unwrap();
    cache.install_link(&ab, 4).unwrap();
    mature(&mut cache, &memory, &c, &ab, 0x1045);
    let abc = cache
        .prepare_transition(&memory, 0x1045, options(), &[])
        .unwrap();
    assert_eq!(
        abc.formation_roots(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1040),
            GuestAddress(0x1045),
            GuestAddress(0x1080),
        ]
    );
    for pc in [0x1000, 0x1005, 0x1040, 0x1045, 0x1080] {
        assert!(abc.entrypoints().contains(&GuestAddress(pc)));
    }
    assert_eq!(
        cache.lookup(&memory, 0x1000).unwrap().identity(),
        abc.identity()
    );
    assert!(a.is_current(&memory) && b.is_current(&memory) && c.is_current(&memory));
    assert_eq!(&cache.link_owners[1..5], &[0, 0, 0, 0]);
    assert!(cache.retained(&ab).is_none() && cache.retained(&c).is_none());
    assert_eq!(cache.snapshot()[5], 1);
}

#[test]
fn a_declined_cap_preserves_the_target_and_requires_new_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(
                &memory,
                0x1040,
                CompileOptions {
                    instructions: 1,
                    ..options()
                },
                &[]
            )
            .is_none()
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        cache.lookup_linked(&memory, 0x1040).unwrap().0.identity(),
        b.identity()
    );
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    mature(&mut cache, &memory, &a, &b, 0x1040);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_some()
    );
}

#[test]
fn an_instruction_union_over_the_limit_cannot_discard_unindexed_interiors() {
    let mut memory = memory();
    let mut leaf = [0x90; 9];
    leaf[8] = 0xc3;
    memory.write(GuestAddress(0x1000), &leaf).unwrap();
    memory.write(GuestAddress(0x1040), &leaf).unwrap();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    assert_eq!((a.instruction_count(), b.instruction_count()), (9, 9));
    assert_eq!(a.entrypoints(), &[GuestAddress(0x1000)]);
    assert_eq!(b.entrypoints(), &[GuestAddress(0x1040)]);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(
                &memory,
                0x1040,
                CompileOptions {
                    instructions: 10,
                    ..options()
                },
                &[],
            )
            .is_none(),
        "two indexed roots do not justify dropping eight existing instruction starts"
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(&cache.link_owners[1..3], &[a.identity(), b.identity()]);
    assert!(cache.retained(&a).is_some() && cache.retained(&b).is_some());
    assert_eq!(
        cache.lookup(&memory, 0x1000).unwrap().identity(),
        a.identity()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1040).unwrap().identity(),
        b.identity()
    );
}

#[test]
fn overlapping_instruction_starts_count_once_against_the_formation_limit() {
    let mut memory = memory();
    let mut leaf = [0x90; 9];
    leaf[8] = 0xc3;
    memory.write(GuestAddress(0x1000), &leaf).unwrap();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1004, 2);
    assert_eq!((a.instruction_count(), b.instruction_count()), (9, 5));
    mature(&mut cache, &memory, &a, &b, 0x1004);
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
    assert_eq!(&cache.link_owners[1..3], &[0, 0]);
    assert!(cache.retained(&a).is_none() && cache.retained(&b).is_none());
}

#[test]
fn preserved_roots_cannot_hide_a_dropped_indexed_interior_entry() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    assert!(a.entrypoints().contains(&GuestAddress(0x1008)));
    assert!(cache.indexes(&a, 0x1008));
    let limited = CompileOptions {
        instructions: 4,
        ..options()
    };
    let candidate = CompiledCode::compile_transition(
        &memory,
        GuestAddress(0x1040),
        limited,
        &[],
        &[],
        &[GuestAddress(0x1000), GuestAddress(0x1040)],
    )
    .unwrap()
    .unwrap();
    assert!(candidate.entrypoints().contains(&GuestAddress(0x1000)));
    assert!(candidate.entrypoints().contains(&GuestAddress(0x1040)));
    assert!(!candidate.entrypoints().contains(&GuestAddress(0x1008)));
    mature(&mut cache, &memory, &a, &b, 0x1040);
    let before = cache.snapshot();
    let entries = cache.entries.clone();
    let owners = cache.link_owners.to_vec();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, limited, &[])
            .is_none(),
        "keeping both roots is insufficient when an indexed interior entry is lost"
    );
    assert!(!cache.transition_pending());
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.entries, entries);
    assert_eq!(cache.link_owners.as_ref(), owners);
    assert!(cache.retained(&a).is_some() && cache.retained(&b).is_some());
    let rejection = cache.retained(&a).unwrap().heat.rejected[0].unwrap();
    assert_eq!(rejection.version, memory.code_source_version());
    assert_eq!(rejection.instructions, limited.instructions);
    assert_eq!(rejection.source_entries, 3);
    assert_eq!(rejection.target_entries, 2);
}

#[test]
fn declined_formation_memo_skips_unchanged_inputs_and_retries_changed_inputs() {
    for change in ["unchanged", "epoch", "entries"] {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let a = insert(&mut cache, &memory, 0x1000, 1);
        let b = insert(&mut cache, &memory, 0x1040, 2);
        let limits = options();
        mature(&mut cache, &memory, &a, &b, 0x1040);
        let rejection = TransitionRejection {
            version: memory.code_source_version(),
            instructions: limits.instructions,
            blocks: limits.blocks,
            wasm_bytes: limits.wasm_bytes,
            source_entries: cache.retained(&a).unwrap().indexed_entries,
            target_entries: cache.retained(&b).unwrap().indexed_entries,
        };
        // this otherwise admissible pair makes a skipped compile observable without a counter.
        cache.regions[a.key.index].as_mut().unwrap().heat.rejected[0] = Some(rejection);
        let mut shadow = None;
        match change {
            "epoch" => {
                memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
                cache.clear_transition();
                mature(&mut cache, &memory, &a, &b, 0x1040);
            }
            "entries" => {
                let owner = insert(&mut cache, &memory, 0x1008, 3);
                assert!(cache.indexes(&owner, 0x1008));
                assert_eq!(cache.retained(&a).unwrap().indexed_entries, 2);
                shadow = Some(owner);
            }
            "unchanged" => {}
            _ => unreachable!(),
        }
        let before = cache.snapshot();
        let entries = cache.entries.clone();
        let coverage = cache.coverage.clone();
        let owners = cache.link_owners.to_vec();
        let mut formed = cache.prepare_transition(&memory, 0x1040, limits, &[]);
        if let Some(shadow) = shadow {
            assert!(
                formed.is_none(),
                "formation cannot steal a third current owner's instruction starts"
            );
            assert!(!cache.transition_pending());
            let retried = cache.retained(&a).unwrap().heat.rejected[0].unwrap();
            assert_eq!(
                retried.source_entries, 2,
                "changed entry counts retry the memoized pair"
            );
            assert!(retried != rejection);
            assert_eq!(cache.snapshot(), before);
            assert_eq!(cache.entries, entries);
            assert_eq!(cache.coverage, coverage);
            assert_eq!(cache.link_owners.as_ref(), owners);
            assert!(cache.retained(&a).is_some() && cache.retained(&b).is_some());
            assert!(cache.retained(&shadow).is_some());
            for (pc, owner) in [(0x1000, &a), (0x1040, &b), (0x1008, &shadow)] {
                assert_eq!(
                    cache.lookup(&memory, pc).unwrap().identity(),
                    owner.identity()
                );
                assert_eq!(
                    cache.coverage_owner(&memory, pc).unwrap().identity(),
                    owner.identity()
                );
                assert!(owner.is_current(&memory));
            }
            continue;
        }
        if change == "unchanged" {
            assert!(formed.is_none());
            assert!(!cache.transition_pending());
            assert_eq!(cache.snapshot(), before);
            assert_eq!(cache.entries, entries);
            assert_eq!(cache.link_owners.as_ref(), owners);
            assert!(cache.retained(&a).unwrap().heat.rejected[0] == Some(rejection));
            cache.regions[a.key.index].as_mut().unwrap().heat.rejected[0] = None;
            mature(&mut cache, &memory, &a, &b, 0x1040);
            formed = cache.prepare_transition(&memory, 0x1040, limits, &[]);
        }
        let formed = formed.unwrap_or_else(|| panic!("{change} must permit a new formation"));
        assert!(formed.is_current(&memory));
        assert!(cache.retained(&a).is_none() && cache.retained(&b).is_none());
        assert_eq!(cache.snapshot()[0], before[0] + 1);
        assert_eq!(&cache.link_owners[1..3], &[0, 0]);
    }
}

#[test]
fn source_epoch_or_identity_changes_cannot_resurrect_pending_owners() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    mature(&mut cache, &memory, &a, &b, 0x1040);
    memory.write(GuestAddress(0x2000), &[0x48]).unwrap();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1040).unwrap().identity(),
        b.identity()
    );
    mature(&mut cache, &memory, &a, &b, 0x1040);
    cache.remove(a.key);
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert_eq!(
        cache.lookup(&memory, 0x1040).unwrap().identity(),
        b.identity()
    );
    let fresh_a = insert(&mut cache, &memory, 0x1000, 1);
    mature(&mut cache, &memory, &fresh_a, &b, 0x1040);
    memory.write(GuestAddress(0x1040), &[0x90]).unwrap();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
    assert!(cache.lookup(&memory, 0x1040).is_none());
    assert!(!b.is_current(&memory));
}

#[test]
fn an_ineligible_self_edge_preserves_a_linked_targets_existing_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = insert(&mut cache, &memory, 0x1000, 1);
    let target = insert(&mut cache, &memory, 0x1040, 2);
    for _ in 0..2 {
        assert!(matches!(
            cache.lookup_transition(&memory, &source, 0x1040, 3),
            TransitionLookup::Linked(ref code, 2) if code.identity() == target.identity()
        ));
    }
    for _ in 0..8 {
        assert!(matches!(
            cache.lookup_transition(&memory, &source, 0x1000, 3),
            TransitionLookup::Linked(ref code, 1) if code.identity() == source.identity()
        ));
        assert!(!cache.transition_pending());
    }
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1040, 3),
        TransitionLookup::Stop
    ));
    let pending = cache.pending_transition.as_ref().unwrap();
    assert_eq!(pending.source, source.key);
    assert_eq!(pending.target, target.key);
    assert_eq!(pending.pc, 0x1040);
}

#[test]
fn alternating_linked_targets_accumulate_independent_promotion_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = insert(&mut cache, &memory, 0x1000, 1);
    let a = insert(&mut cache, &memory, 0x1040, 2);
    let b = insert(&mut cache, &memory, 0x1080, 3);
    let before = cache.snapshot();
    for _ in 1..TRANSITION_HITS {
        assert!(!cache.observe_transition(&memory, &source, &a, 0x1040, TRANSITION_HITS));
        assert!(!cache.observe_transition(&memory, &source, &b, 0x1080, TRANSITION_HITS));
    }
    assert!(cache.observe_transition(&memory, &source, &a, 0x1040, TRANSITION_HITS));
    assert!(cache.transition_pending());
    assert!(
        !cache.observe_transition(&memory, &source, &b, 0x1080, TRANSITION_HITS),
        "only one promotion may be pending"
    );
    cache.clear_transition();
    assert!(cache.observe_transition(&memory, &source, &b, 0x1080, TRANSITION_HITS));
    cache.clear_transition();
    assert!(!cache.observe_transition(&memory, &source, &a, 0x1040, TRANSITION_HITS));
    assert_eq!(cache.snapshot(), before, "observation alone admits no code");
    assert_eq!(
        &cache.link_owners[1..4],
        &[source.identity(), a.identity(), b.identity()]
    );
}

#[test]
fn evicted_targets_cannot_pin_high_heat_against_two_new_hot_exits() {
    let mut memory = memory();
    for pc in [0x2000, 0x2010, 0x2020, 0x2030, 0x2040, 0x2050] {
        memory.write(GuestAddress(pc), &[0x40, 0xc3]).unwrap();
    }
    let mut cache = CoreCache32::default();
    let source = insert(&mut cache, &memory, 0x1000, 1);
    let old: Vec<_> = [0x2000, 0x2010, 0x2020, 0x2030]
        .into_iter()
        .enumerate()
        .map(|(index, pc)| (insert(&mut cache, &memory, pc, index as u32 + 2), pc))
        .collect();
    for (target, pc) in &old {
        for _ in 0..3 {
            assert!(!cache.observe_transition(&memory, &source, target, *pc, 4));
        }
    }
    for (target, _) in &old {
        cache.remove(target.key);
        assert!(target.is_current(&memory));
    }
    let _unrelated = [
        insert(&mut cache, &memory, 0x2040, 6),
        insert(&mut cache, &memory, 0x2050, 7),
    ];
    let a = insert(&mut cache, &memory, 0x1040, 2);
    let b = insert(&mut cache, &memory, 0x1080, 3);
    for (target, _) in &old {
        assert!(
            cache.regions[target.key.index].is_some(),
            "all old indices were reused"
        );
    }
    for _ in 0..3 {
        assert!(!cache.observe_transition(&memory, &source, &a, 0x1040, 4));
        assert!(!cache.observe_transition(&memory, &source, &b, 0x1080, 4));
    }
    assert!(cache.observe_transition(&memory, &source, &a, 0x1040, 4));
    cache.clear_transition();
    assert!(cache.observe_transition(&memory, &source, &b, 0x1080, 4));
}

#[test]
fn distinct_entries_in_one_target_keep_independent_edge_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = insert(&mut cache, &memory, 0x1000, 1);
    let target = insert(&mut cache, &memory, 0x1040, 2);
    for _ in 0..2 {
        assert!(!cache.observe_transition(&memory, &source, &target, 0x1040, 3));
    }
    assert!(!cache.observe_transition(&memory, &source, &target, 0x1045, 3));
    assert!(cache.observe_transition(&memory, &source, &target, 0x1040, 3));
    cache.clear_transition();
    assert!(!cache.observe_transition(&memory, &source, &target, 0x1045, 3));
    assert!(cache.observe_transition(&memory, &source, &target, 0x1045, 3));
}

#[test]
fn a_replaced_target_cannot_inherit_a_held_targets_edge_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = insert(&mut cache, &memory, 0x1000, 1);
    let old = insert(&mut cache, &memory, 0x1040, 2);
    for _ in 0..2 {
        assert!(!cache.observe_transition(&memory, &source, &old, 0x1040, 3));
    }
    let target = insert(&mut cache, &memory, 0x1040, 2);
    assert!(old.is_current(&memory));
    assert!(!cache.observe_transition(&memory, &source, &old, 0x1040, 3));
    assert!(!cache.observe_transition(&memory, &source, &target, 0x1040, 3));
    assert!(!cache.observe_transition(&memory, &source, &target, 0x1040, 3));
    assert!(cache.observe_transition(&memory, &source, &target, 0x1040, 3));
}

#[test]
fn only_the_current_linked_indexed_pair_can_accumulate_heat() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, &memory, 0x1000, 1);
    let b = insert(&mut cache, &memory, 0x1040, 2);
    let c = insert(&mut cache, &memory, 0x1080, 3);
    assert!(!cache.observe_transition(&memory, &a, &a, 0x1000, 1));
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1041, 1));
    cache.unlink(2, b.identity()).unwrap();
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1040, 1));
    cache.install_link(&b, 2).unwrap();
    assert!(!cache.observe_transition(&memory, &a, &b, 0x1040, 2));
    assert!(!cache.observe_transition(&memory, &a, &c, 0x1080, 2));
    assert!(cache.observe_transition(&memory, &a, &b, 0x1040, 2));
    assert!(!cache.observe_transition(&memory, &a, &c, 0x1080, 2));
    cache.clear_transition();
    assert!(cache.observe_transition(&memory, &a, &c, 0x1080, 2));
    cache.clear_transition();
    assert!(
        cache
            .prepare_transition(&memory, 0x1040, options(), &[])
            .is_none()
    );
}

#[test]
fn forming_a_new_ordinal_table_does_not_reset_the_persistent_numerical_bank() {
    let mut instance = EngineInstance::new(2, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x3000, 1, 3).unwrap();
    let memory = instance.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(0x1000), &[0xd9, 0x03, 0xff, 0xe6])
        .unwrap();
    memory
        .write(
            GuestAddress(0x1040),
            &[0xd8, 0x4b, 4, 0xd9, 0x5b, 8, 0xff, 0xe2],
        )
        .unwrap();
    memory
        .write(GuestAddress(0x3000), &3_f32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x3004), &2_f32.to_le_bytes())
        .unwrap();
    let mut cache = CoreCache32::default();
    let a = insert(&mut cache, memory, 0x1000, 1);
    let b = insert(&mut cache, memory, 0x1040, 2);
    assert_eq!(a.native_instructions()[0].pc, 0x1000);
    assert_eq!(b.native_instructions()[0].pc, 0x1040);
    let mut bank = NumericBank::from_instance(&instance).unwrap();
    let mut context = crate::cpu::core::Context32::new(State32 {
        registers: [0, 0, 0x1000, 0x3000, 0, 0, 0x1040, 0],
        eip: 0x1000,
        ..State32::default()
    });
    assert_eq!(
        bank.execute(&mut instance, &mut context, &a.native_instructions()[0]),
        0
    );
    let state = bank.architectural_state();
    let arena = instance.arena().to_vec();
    let context_before = *context.bytes();
    let memory = instance.memory.as_ref().unwrap();
    mature(&mut cache, memory, &a, &b, 0x1040);
    let merged = cache
        .prepare_transition(memory, 0x1040, options(), &[])
        .unwrap();
    assert_eq!(bank.architectural_state(), state);
    assert_eq!(instance.arena(), arena);
    assert_eq!(*context.bytes(), context_before);
    assert_eq!(
        merged
            .native_instructions()
            .iter()
            .map(|op| op.pc)
            .collect::<Vec<_>>(),
        [0x1000, 0x1040, 0x1043]
    );
    for operation in &merged.native_instructions()[1..] {
        assert_eq!(bank.execute(&mut instance, &mut context, operation), 0);
    }
    let final_state = bank.architectural_state().0;
    assert_eq!(final_state.tag, 0xffff);
    assert_eq!(
        final_state.registers[7],
        [0, 0, 0, 0, 0, 0, 0, 0xc0, 1, 0x40]
    );
    assert_eq!(
        (final_state.instruction_pointer, final_state.data_pointer),
        (0x1043, 0x3008)
    );
    let mut output = [0; 4];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x3008), &mut output)
        .unwrap();
    assert_eq!(output, 6_f32.to_le_bytes());
}

fn cross_page_call_memory() -> AddressSpace {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x0f, 0, 0])
        .unwrap();
    memory
}

#[test]
fn cross_page_call_and_return_pairs_retain_both_roots_and_the_continuation() {
    for return_edge in [false, true] {
        let memory = cross_page_call_memory();
        let mut cache = CoreCache32::default();
        let caller = insert(&mut cache, &memory, 0x1000, 1);
        let callee = insert(&mut cache, &memory, 0x2000, 2);
        let before = cache.snapshot();
        let (source, target, pc) = if return_edge {
            (&callee, &caller, 0x1005)
        } else {
            (&caller, &callee, 0x2000)
        };
        mature(&mut cache, &memory, source, target, pc);
        assert_eq!(cache.snapshot(), before);
        let merged = cache
            .prepare_transition(&memory, pc, options(), &[])
            .unwrap();
        let expected_roots = if return_edge {
            vec![
                GuestAddress(0x1000),
                GuestAddress(0x1005),
                GuestAddress(0x2000),
            ]
        } else {
            vec![GuestAddress(0x1000), GuestAddress(0x2000)]
        };
        assert_eq!(merged.formation_roots(), expected_roots);
        assert_eq!(merged.instruction_count(), 6);
        for entry in [0x1000, 0x1005, 0x2000] {
            assert!(merged.entrypoints().contains(&GuestAddress(entry)));
            assert_eq!(
                cache.lookup(&memory, entry).unwrap().identity(),
                merged.identity()
            );
        }
        assert_ne!(merged.identity(), caller.identity());
        assert_ne!(merged.identity(), callee.identity());
        assert_eq!(&cache.link_owners[1..3], &[0, 0]);
        assert!(cache.retained(&caller).is_none() && cache.retained(&callee).is_none());
        assert!(caller.is_current(&memory) && callee.is_current(&memory));
        assert!(merged.is_current(&memory));
        assert_eq!(cache.snapshot()[0], before[0] + 1);
        assert_eq!(cache.snapshot()[5], 1);
        assert_eq!(cache.snapshot()[6], merged.wasm().len() as u64);
    }
}

#[test]
fn cross_page_formed_sources_reject_writes_and_permission_changes_on_either_page() {
    for changed_page in [0x1000, 0x2000] {
        for revoke_execute in [false, true] {
            let mut memory = cross_page_call_memory();
            let mut cache = CoreCache32::default();
            let caller = insert(&mut cache, &memory, 0x1000, 1);
            let callee = insert(&mut cache, &memory, 0x2000, 2);
            mature(&mut cache, &memory, &caller, &callee, 0x2000);
            let merged = cache
                .prepare_transition(&memory, 0x2000, options(), &[])
                .unwrap();
            assert!(merged.is_current(&memory));
            if revoke_execute {
                memory
                    .protect(
                        PageRange::new(GuestAddress(changed_page), 1).unwrap(),
                        Permissions::READ,
                    )
                    .unwrap();
            } else {
                memory.write(GuestAddress(changed_page), &[0x90]).unwrap();
            }
            assert!(!merged.is_current(&memory));
            assert_eq!(caller.is_current(&memory), changed_page != 0x1000);
            assert_eq!(callee.is_current(&memory), changed_page != 0x2000);
            assert!(cache.lookup(&memory, 0x2000).is_none());
            assert_eq!(&cache.link_owners[1..3], &[0, 0]);
            assert!(cache.retained(&caller).is_none() && cache.retained(&callee).is_none());
        }
    }
}

#[test]
fn cross_page_shortening_declines_without_admission_and_requires_fresh_heat() {
    let memory = cross_page_call_memory();
    let mut cache = CoreCache32::default();
    let caller = insert(&mut cache, &memory, 0x1000, 1);
    let callee = insert(&mut cache, &memory, 0x2000, 2);
    mature(&mut cache, &memory, &caller, &callee, 0x2000);
    let before = cache.snapshot();
    assert!(
        cache
            .prepare_transition(
                &memory,
                0x2000,
                CompileOptions {
                    instructions: 1,
                    ..options()
                },
                &[],
            )
            .is_none()
    );
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        cache.lookup_linked(&memory, 0x2000).unwrap().0.identity(),
        callee.identity()
    );
    assert_eq!(
        &cache.link_owners[1..3],
        &[caller.identity(), callee.identity()]
    );
    assert!(
        cache
            .prepare_transition(&memory, 0x2000, options(), &[])
            .is_none()
    );
    mature(&mut cache, &memory, &caller, &callee, 0x2000);
    assert!(
        cache
            .prepare_transition(&memory, 0x2000, options(), &[])
            .is_some()
    );
}
