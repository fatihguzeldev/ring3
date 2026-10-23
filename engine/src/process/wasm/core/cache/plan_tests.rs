use super::*;
use crate::{
    cpu::{
        core::{
            NativeInstruction, compile, emit_observed_plan, prepare_observed_plan,
            trace::{CompletedColdObservation, PendingColdObservation},
        },
        x86::decode::{BoundDecode, DecodeAuthority, DecodePolicy, decode_for_compilation},
    },
    memory::{PageRange, Permissions},
};

const ROOT: u32 = 0x1000;
const CALLEE: u32 = 0x2000;
const OTHER: u32 = 0x3000;

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for pc in [ROOT, CALLEE, OTHER] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    // call2000; inc eax; jmp esi. the actual retained return is1005.
    memory
        .write(
            GuestAddress(ROOT),
            &[0xe8, 0xfb, 0x0f, 0, 0, 0x40, 0xff, 0xe6],
        )
        .unwrap();
    memory.write(GuestAddress(CALLEE), &[0x41, 0xc3]).unwrap();
    memory.write(GuestAddress(OTHER), &[0x90, 0x90]).unwrap();
    memory
}

fn completed(memory: &AddressSpace, pc: u32, post: u32) -> CompletedColdObservation {
    let BoundDecode::Instruction(decoded) = decode_for_compilation(
        memory,
        GuestAddress(pc),
        DecodePolicy::Execution,
        None,
        DecodeAuthority::default(),
    )
    .unwrap() else {
        panic!("the fixture contains an actual supported instruction")
    };
    PendingColdObservation::capture(
        memory,
        &NativeInstruction {
            pc,
            next: decoded.next_pc().0,
            operation: *decoded.operation(),
        },
    )
    .unwrap()
    .complete(memory, post)
    .unwrap()
}

fn observed_plan(
    memory: &AddressSpace,
    path: &[(u32, u32)],
    parents: &[&CompiledCode],
    options: CompileOptions,
) -> ObservedPlan {
    let cold: Vec<_> = path
        .iter()
        .map(|&(pc, post)| completed(memory, pc, post))
        .collect();
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(index, item)| item.event(index).unwrap())
        .collect();
    prepare_observed_plan(
        memory,
        GuestAddress(path[0].0),
        options,
        &events,
        parents,
        memory.code_source_version(),
        &cold,
        parents,
    )
    .unwrap()
    .unwrap()
}

fn emit(memory: &AddressSpace, plan: ObservedPlan) -> CompiledCode {
    emit_observed_plan(memory, plan).unwrap().unwrap()
}

fn two_families(memory: &AddressSpace) -> (CoreCache32, RegionRef, RegionRef) {
    let mut cache = CoreCache32::default();
    let caller = emit(
        memory,
        observed_plan(memory, &[(ROOT, CALLEE)], &[], CompileOptions::default()),
    );
    let caller = cache.admit(memory, caller).unwrap();
    let callee = emit(
        memory,
        observed_plan(
            memory,
            &[(CALLEE, CALLEE + 1), (CALLEE + 1, ROOT + 5)],
            &[],
            CompileOptions::default(),
        ),
    );
    let callee = cache.admit(memory, callee).unwrap();
    cache.install_link(&caller, 7).unwrap();
    cache.install_link(&callee, 8).unwrap();
    (cache, caller, callee)
}

fn connected_plan(
    memory: &AddressSpace,
    parents: &[&CompiledCode],
    options: CompileOptions,
) -> ObservedPlan {
    observed_plan(
        memory,
        &[
            (ROOT, CALLEE),
            (CALLEE, CALLEE + 1),
            (CALLEE + 1, ROOT + 5),
            (ROOT + 5, ROOT + 6),
            (ROOT + 6, ROOT),
        ],
        parents,
        options,
    )
}

fn resources(cache: &CoreCache32) {
    let count = cache.regions.iter().flatten().count();
    let bytes: usize = cache
        .regions
        .iter()
        .flatten()
        .map(|entry| entry.code.wasm().len())
        .sum();
    assert_eq!(cache.snapshot()[5], count as u64);
    assert_eq!(cache.snapshot()[6], bytes as u64);
    assert_eq!(cache.bytes, bytes);
    assert_eq!(cache.free.len() + count, MAX_REGIONS);
    assert!(bytes <= MAX_CODE_BYTES);
}

#[test]
fn connected_current_families_preflight_before_emission_then_replace_every_alias_atomically() {
    let memory = memory();
    let (mut cache, caller, callee) = two_families(&memory);
    let baseline = cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(ROOT),
                CompileOptions {
                    instructions: 1,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    cache.install_link(&baseline, 1).unwrap();
    let coverage = cache.coverage.clone();
    let entries = cache.entries.clone();
    let baseline_bytes = baseline.wasm().to_vec();
    let before = cache.snapshot();
    let before_owners = cache.link_owners.to_vec();
    let before_order = (cache.oldest, cache.newest);
    let plan = connected_plan(&memory, &[&callee, &caller], CompileOptions::default());
    let admission = cache.preflight_observed(&memory, &plan).unwrap();
    assert_eq!(cache.snapshot(), before);
    assert_eq!(&*cache.link_owners, before_owners);
    assert_eq!((cache.oldest, cache.newest), before_order);
    let next = emit(&memory, plan);
    let encoded = next.wasm().len();
    let next = cache.commit_observed(&memory, admission, next).unwrap();
    assert!(cache.retained(&caller).is_none());
    assert!(cache.retained(&callee).is_none());
    assert_eq!(&cache.link_owners[7..9], &[0, 0]);
    cache.install_link(&next, 8).unwrap();
    for old in [&caller, &callee] {
        for pc in old.instruction_pcs() {
            let (owner, slot) = cache.lookup_linked(&memory, pc).unwrap();
            assert!(Rc::ptr_eq(&owner.code, &next.code));
            assert_eq!(slot, 8);
        }
        assert_eq!(cache.unlink(8, old.identity()), Err(LinkError::WrongOwner));
    }
    for pc in [ROOT, ROOT + 5, ROOT + 6, CALLEE, CALLEE + 1] {
        assert_eq!(
            cache.lookup(&memory, pc).unwrap().identity(),
            next.identity()
        );
    }
    assert_eq!(cache.coverage, coverage);
    assert_eq!(cache.entries, entries);
    assert_eq!(baseline.wasm(), baseline_bytes);
    assert_eq!(cache.link_owners[1], baseline.identity());
    assert_eq!(cache.snapshot()[0], before[0] + 1);
    assert_eq!(cache.snapshot()[1], before[1] + encoded as u64);
    assert_eq!(cache.snapshot()[5], before[5] - 1);
    assert_eq!(
        cache.snapshot()[6],
        before[6] - caller.wasm().len() as u64 - callee.wasm().len() as u64 + encoded as u64
    );
    assert_eq!(cache.snapshot()[2..5], before[2..5]);
    resources(&cache);
}

#[test]
fn missing_current_parent_is_refused_neutrally_and_unchanged_plan_feedback_survives_link_activity()
{
    let memory = memory();
    let (mut cache, caller, callee) = two_families(&memory);
    // the latest actual path happens to cover both owners, but only caller's
    // entire historical graph was authorized as a parent.
    let plan = connected_plan(&memory, &[&caller], CompileOptions::default());
    let before = cache.snapshot();
    let aliases = cache.trace_entries.clone();
    let slots = cache.link_owners.to_vec();
    let epoch = cache.ownership_epoch;
    assert!(cache.preflight_observed(&memory, &plan).is_none());
    assert_eq!(cache.observed_refusals.len(), 1);
    cache.bypass_trace(caller.identity());
    cache.unlink(8, callee.identity()).unwrap();
    cache.install_link(&callee, 8).unwrap();
    cache.bypass_trace(0);
    assert_eq!(cache.ownership_epoch, epoch);
    assert!(cache.preflight_observed(&memory, &plan).is_none());
    assert_eq!(cache.observed_refusals.len(), 1);
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.trace_entries, aliases);
    assert_eq!(&*cache.link_owners, slots);
    cache.remove(callee.key);
    assert!(
        cache.preflight_observed(&memory, &plan).is_some(),
        "ownership change releases exact old rejection feedback"
    );
    assert!(cache.retained(&caller).is_some());
    resources(&cache);
}

#[test]
fn source_change_after_preflight_or_emission_cannot_remove_any_parent() {
    let mut memory = memory();
    let (mut cache, caller, callee) = two_families(&memory);
    let plan = connected_plan(&memory, &[&caller, &callee], CompileOptions::default());
    let admission = cache.preflight_observed(&memory, &plan).unwrap();
    let code = emit(&memory, plan);
    let before = cache.snapshot();
    let aliases = cache.trace_entries.clone();
    let slots = cache.link_owners.to_vec();
    memory.write(GuestAddress(CALLEE), &[0x41]).unwrap();
    assert!(matches!(
        cache.commit_observed(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.trace_entries, aliases);
    assert_eq!(&*cache.link_owners, slots);
    assert!(cache.retained(&caller).is_some());
    assert!(cache.retained(&callee).is_some());
    resources(&cache);
}

#[test]
fn intervening_eviction_and_slot_aba_refuse_the_old_admission_without_touching_survivors() {
    let memory = memory();
    let (mut cache, caller, callee) = two_families(&memory);
    let plan = connected_plan(&memory, &[&caller, &callee], CompileOptions::default());
    let admission = cache.preflight_observed(&memory, &plan).unwrap();
    let code = emit(&memory, plan);
    cache.remove(callee.key);
    let other = emit(
        &memory,
        observed_plan(
            &memory,
            &[(OTHER, OTHER + 1)],
            &[],
            CompileOptions::default(),
        ),
    );
    let other = cache.admit(&memory, other).unwrap();
    cache.install_link(&other, 8).unwrap();
    let before = cache.snapshot();
    let aliases = cache.trace_entries.clone();
    let slots = cache.link_owners.to_vec();
    assert!(matches!(
        cache.commit_observed(&memory, admission, code),
        Err(CompileError::InvalidLimits)
    ));
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.trace_entries, aliases);
    assert_eq!(&*cache.link_owners, slots);
    assert!(cache.retained(&caller).is_some());
    assert!(cache.retained(&other).is_some());
    assert_eq!(
        cache.unlink(8, callee.identity()),
        Err(LinkError::WrongOwner)
    );
    resources(&cache);
}

#[test]
fn stale_overlap_is_removed_only_after_fresh_plan_and_code_are_accepted() {
    let mut memory = memory();
    let (mut cache, caller, callee) = two_families(&memory);
    memory.write(GuestAddress(CALLEE), &[0x41]).unwrap();
    let fresh = observed_plan(
        &memory,
        &[(CALLEE, CALLEE + 1), (CALLEE + 1, ROOT + 5)],
        &[],
        CompileOptions::default(),
    );
    let before = cache.snapshot();
    let admission = cache.preflight_observed(&memory, &fresh).unwrap();
    assert_eq!(cache.snapshot(), before);
    assert_eq!(cache.link_owners[8], callee.identity());
    let next = cache
        .commit_observed(&memory, admission, emit(&memory, fresh))
        .unwrap();
    assert!(cache.retained(&callee).is_none());
    assert!(cache.retained(&caller).is_some());
    assert_eq!(cache.link_owners[8], 0);
    assert_eq!(cache.snapshot()[4], before[4] + 1);
    assert_eq!(
        cache.lookup(&memory, CALLEE + 1).unwrap().identity(),
        next.identity()
    );
    resources(&cache);
}

#[test]
fn exact_emit_refusal_is_bounded_and_different_limits_remain_eligible() {
    let memory = memory();
    let (mut cache, caller, callee) = two_families(&memory);
    let before = cache.snapshot();
    for bytes in 128..128 + MAX_OBSERVED_REFUSALS + 2 {
        let options = CompileOptions {
            wasm_bytes: bytes,
            ..CompileOptions::default()
        };
        let plan = connected_plan(&memory, &[&caller, &callee], options);
        let admission = cache.preflight_observed(&memory, &plan).unwrap();
        assert!(
            emit_observed_plan(&memory, plan).unwrap().is_none(),
            "the actual backend exceeds this authored byte bound"
        );
        cache.reject_observed(admission);
        let repeated = connected_plan(&memory, &[&caller, &callee], options);
        assert!(cache.preflight_observed(&memory, &repeated).is_none());
        assert!(cache.observed_refusals.len() <= MAX_OBSERVED_REFUSALS);
        assert_eq!(cache.snapshot(), before);
    }
    assert_eq!(cache.observed_refusals.len(), MAX_OBSERVED_REFUSALS);
    let plan = connected_plan(&memory, &[&caller, &callee], CompileOptions::default());
    assert!(cache.preflight_observed(&memory, &plan).is_some());
    resources(&cache);
}

#[test]
fn frontier_query_ignores_runtime_bypass_and_does_not_mutate_source_or_lru() {
    let memory = memory();
    let (mut cache, caller, _) = two_families(&memory);
    cache
        .admit(
            &memory,
            compile(
                &memory,
                GuestAddress(OTHER),
                CompileOptions {
                    instructions: 2,
                    blocks: 1,
                    ..CompileOptions::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
    let before = cache.snapshot();
    let order = (cache.oldest, cache.newest);
    cache.bypass_trace(caller.identity());
    assert!(cache.lookup(&memory, ROOT).is_none());
    assert!(cache.owned_frontier_contains(&memory, ROOT));
    assert!(cache.owned_frontier_contains(&memory, CALLEE + 1));
    assert!(!cache.entries.contains_key(&(OTHER + 1)));
    assert!(
        cache.owned_frontier_contains(&memory, OTHER + 1),
        "an unadvertised baseline interior is still owned source coverage"
    );
    assert!(!cache.owned_frontier_contains(&memory, ROOT + 1));
    assert!(!cache.owned_frontier_contains(&memory, ROOT + 5));
    assert_eq!(cache.snapshot(), before);
    assert_eq!((cache.oldest, cache.newest), order);
}
