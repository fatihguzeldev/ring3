use super::*;
use crate::{
    cpu::core::trace::{CompletedColdObservation, PendingColdObservation},
    cpu::{
        core::{NativeInstruction, compile, compile_observed_with_cold},
        x86::ir::{Condition, Operation},
    },
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x75, 0x02, 0x90, 0x90, 0x90, 0x90])
        .unwrap();
    memory
}

fn observation(memory: &AddressSpace, pc: u32, post: u32) -> CompletedColdObservation {
    let instruction = if pc == 0x1000 {
        NativeInstruction {
            pc,
            next: pc + 2,
            operation: Operation::ConditionalJump {
                condition: Condition::NotEqual,
                target: GuestAddress(0x1004),
            },
        }
    } else {
        NativeInstruction {
            pc,
            next: pc + 1,
            operation: Operation::Nop,
        }
    };
    PendingColdObservation::capture(memory, &instruction)
        .unwrap()
        .complete(memory, post)
        .unwrap()
}

fn family(memory: &AddressSpace, arm: u32, previous: Option<&CompiledCode>) -> CompiledCode {
    let cold = [
        observation(memory, 0x1000, arm),
        observation(memory, arm, arm + 1),
    ];
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(index, record)| record.event(index).unwrap())
        .collect();
    compile_observed_with_cold(
        memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[],
        memory.code_source_version(),
        &cold,
        previous,
    )
    .unwrap()
    .unwrap()
}

fn invariants(cache: &CoreCache32, code: &RegionRef) {
    let entry = cache.retained(code).unwrap();
    assert_eq!(entry.admitted_entries, code.entrypoints());
    assert_eq!(entry.indexed_entries, code.instruction_count());
    for pc in code.instruction_pcs() {
        assert_eq!(cache.trace_entries[&pc], code.key);
    }
    assert_eq!(
        cache.snapshot()[5],
        cache.regions.iter().flatten().count() as u64
    );
    assert_eq!(
        cache.snapshot()[6],
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.code.wasm().len() as u64)
            .sum::<u64>()
    );
    assert_eq!(cache.coverage.len(), 0);
    assert_eq!(cache.entries.len(), 0);
}

#[test]
fn every_validated_family_port_shares_one_identity_slot_byte_budget_and_lru_node() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let code = cache.admit(&memory, family(&memory, 0x1002, None)).unwrap();
    cache.install_link(&code, 7).unwrap();
    let before = cache.snapshot();
    for pc in [0x1000, 0x1002] {
        let owner = cache.observed_owner(&memory, pc).unwrap();
        assert!(Rc::ptr_eq(&owner.code, &code.code));
        let (linked, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(Rc::ptr_eq(&linked.code, &code.code));
        assert_eq!(slot, 7);
        assert_eq!(
            cache.lookup(&memory, pc).unwrap().identity(),
            code.identity()
        );
    }
    assert!(cache.lookup(&memory, 0x1001).is_none());
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.oldest(), cache.newest());
    invariants(&cache, &code);
    cache.bypass_trace(code.identity());
    assert!(cache.lookup(&memory, 0x1000).is_none());
    assert!(cache.lookup_linked(&memory, 0x1002).is_none());
    cache.bypass_trace(0);
    assert_eq!(
        cache.lookup(&memory, 0x1002).unwrap().identity(),
        code.identity()
    );
}

#[test]
fn exact_growth_atomically_replaces_all_aliases_and_old_slot_identity_cannot_aba() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.admit(&memory, family(&memory, 0x1002, None)).unwrap();
    cache.install_link(&first, 7).unwrap();
    let next_code = family(&memory, 0x1004, Some(&first));
    assert!(next_code.observed_grows(&first));
    let next = cache.admit(&memory, next_code).unwrap();
    assert!(cache.retained(&first).is_none());
    assert_eq!(cache.link_owners[7], 0);
    cache.install_link(&next, 7).unwrap();
    assert_eq!(
        cache.unlink(7, first.identity()),
        Err(LinkError::WrongOwner)
    );
    for pc in [0x1000, 0x1002, 0x1004] {
        assert_eq!(
            cache.lookup_linked(&memory, pc).unwrap().0.identity(),
            next.identity()
        );
    }
    assert_eq!(cache.snapshot()[0], 2);
    assert_eq!(cache.snapshot()[5], 1);
    invariants(&cache, &next);
}

#[test]
fn overlapping_distinct_current_family_is_rejected_without_stealing_any_alias() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.admit(&memory, family(&memory, 0x1002, None)).unwrap();
    let bytes = first.wasm().to_vec();
    let before = cache.snapshot();
    assert!(matches!(
        cache.admit(&memory, family(&memory, 0x1004, None)),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(first.wasm(), bytes);
    assert_eq!(
        cache.lookup(&memory, 0x1002).unwrap().identity(),
        first.identity()
    );
    assert!(cache.lookup(&memory, 0x1004).is_none());
    invariants(&cache, &first);
}

#[test]
fn stale_source_removes_whole_family_alias_directory_and_releases_its_single_slot() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let code = cache.admit(&memory, family(&memory, 0x1002, None)).unwrap();
    cache.install_link(&code, 7).unwrap();
    memory.write(GuestAddress(0x1000), &[0x75]).unwrap();
    assert!(cache.observed_owner(&memory, 0x1002).is_none());
    assert!(cache.trace_entries.is_empty());
    assert_eq!(cache.link_owners[7], 0);
    assert_eq!(cache.snapshot()[4], 1);
    assert_eq!(cache.snapshot()[5..7], [0, 0]);
}

#[test]
fn family_priority_preserves_explicit_legacy_baseline_coverage_and_bytes() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let baseline = cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(0x1000),
                CompileOptions {
                    instructions: 2,
                    blocks: 2,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    let coverage = cache.coverage.clone();
    let entries = cache.entries.clone();
    let bytes = baseline.wasm().to_vec();
    let graph = cache.admit(&memory, family(&memory, 0x1002, None)).unwrap();
    assert_eq!(
        cache.lookup(&memory, 0x1000).unwrap().identity(),
        graph.identity()
    );
    assert_eq!(cache.coverage, coverage);
    assert_eq!(cache.entries, entries);
    assert_eq!(baseline.wasm(), bytes);
    assert!(cache.retained(&baseline).is_some());
}

#[test]
fn a_larger_legacy_root_only_trace_cannot_downgrade_a_current_family() {
    use crate::cpu::core::{
        compile_observed, compile_with_frontier,
        trace::{BLOCK, TraceEvent},
    };
    let memory = memory();
    let mut cache = CoreCache32::default();
    let family = cache.admit(&memory, family(&memory, 0x1002, None)).unwrap();
    let prefix = compile_with_frontier(
        &memory,
        GuestAddress(0x1000),
        CompileOptions {
            instructions: 8,
            blocks: 2,
            ..CompileOptions::default()
        },
        &[],
        &[GuestAddress(0x1004)],
    )
    .unwrap();
    let witness = compile(
        &memory,
        GuestAddress(0x1004),
        CompileOptions {
            instructions: 1,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let events = [
        TraceEvent {
            kind: BLOCK,
            pc: 0x1000,
            end: 0x1002,
            offset: 0,
            owner: prefix.identity(),
        },
        TraceEvent {
            kind: BLOCK,
            pc: 0x1002,
            end: 0x1004,
            offset: 0,
            owner: prefix.identity(),
        },
        TraceEvent {
            kind: BLOCK,
            pc: 0x1004,
            end: 0x1005,
            offset: 0,
            owner: witness.identity(),
        },
    ];
    let legacy = compile_observed(
        &memory,
        GuestAddress(0x1000),
        CompileOptions::default(),
        &events,
        &[&prefix, &witness],
        memory.code_source_version(),
    )
    .unwrap()
    .unwrap();
    assert!(!legacy.is_observed_family());
    assert!(
        legacy
            .trace_score()
            .unwrap()
            .strictly_grows(family.trace_score().unwrap())
    );
    let before = cache.snapshot();
    assert!(matches!(
        cache.admit(&memory, legacy),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(
        cache.lookup(&memory, 0x1002).unwrap().identity(),
        family.identity()
    );
    invariants(&cache, &family);
}
