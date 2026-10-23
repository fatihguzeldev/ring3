use super::*;
use crate::{
    cpu::{
        core::{
            NativeInstruction, emit_observed_plan, prepare_observed_plan,
            trace::{CompletedColdObservation, PendingColdObservation},
        },
        x86::decode::{BoundDecode, DecodeAuthority, DecodePolicy, decode_for_compilation},
    },
    memory::{PageRange, Permissions},
};
use std::hash::BuildHasher;

// A full probe run makes DELETED removal deterministic, rather than depending
// on the random hasher happening to cluster a small linear instruction stream.
// Every key is still a real, non-overlapping guest JMP ESI instruction.
fn collision_memory(cache: &CoreCache32) -> (AddressSpace, Vec<u32>) {
    let mut buckets: Vec<Vec<u32>> = (0..256).map(|_| Vec::new()).collect();
    for pc in (0x1000..0x41000u32).step_by(4) {
        let bucket = cache.trace_entries.hasher().hash_one(pc) as usize & 255;
        buckets[bucket].push(pc);
    }
    let pcs: Vec<_> = buckets
        .into_iter()
        .max_by_key(Vec::len)
        .unwrap()
        .into_iter()
        .take(113)
        .collect();
    assert_eq!(pcs.len(), 113);
    let mut memory = AddressSpace::new(65).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 64).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x50000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for pc in &pcs {
        memory.write(GuestAddress(*pc), &[0xff, 0xe6]).unwrap();
    }
    memory
        .write(GuestAddress(0x50000), &[0x90, 0xff, 0xe7])
        .unwrap();
    (memory, pcs)
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
        panic!("the fixture contains a supported indirect jump")
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

fn plan(memory: &AddressSpace, path: &[(u32, u32)], parents: &[&CompiledCode]) -> ObservedPlan {
    let cold: Vec<_> = path
        .iter()
        .map(|&(pc, post)| completed(memory, pc, post))
        .collect();
    let events: Vec<_> = cold
        .iter()
        .enumerate()
        .map(|(i, item)| item.event(i).unwrap())
        .collect();
    prepare_observed_plan(
        memory,
        GuestAddress(path[0].0),
        CompileOptions::default(),
        &events,
        parents,
        memory.code_source_version(),
        &cold,
        parents,
    )
    .unwrap()
    .unwrap()
}

fn family(memory: &AddressSpace, pcs: &[u32]) -> CompiledCode {
    let path: Vec<_> = pcs
        .iter()
        .enumerate()
        .map(|(i, pc)| (*pc, pcs[(i + 1) % pcs.len()]))
        .collect();
    emit_observed_plan(memory, plan(memory, &path, &[]))
        .unwrap()
        .unwrap()
}

#[test]
fn deleted_family_aliases_do_not_credit_unreleased_hash_storage() {
    let mut cache = CoreCache32::default();
    let (memory, pcs) = collision_memory(&cache);
    let first = cache.admit(&memory, family(&memory, &pcs[..56])).unwrap();
    let second = cache
        .admit(&memory, family(&memory, &pcs[56..112]))
        .unwrap();
    let third = cache.admit(&memory, family(&memory, &pcs[112..])).unwrap();
    cache.install_link(&first, 7).unwrap();
    cache.install_link(&second, 8).unwrap();
    cache.install_link(&third, 9).unwrap();
    assert_eq!(cache.trace_entries.len(), 113);
    assert_eq!(cache.trace_entries.capacity(), 224);
    let before_directory = cache.directory_charge();
    let before_resident = cache.resident_charge();
    let first_charge = cache.region(first.key).unwrap().charge;
    cache.remove(first.key);
    assert!(
        cache.trace_entries.capacity() < 224,
        "the actual deletion must leave tombstones"
    );
    assert_eq!(
        cache.directory_charge(),
        before_directory,
        "removing aliases freed no hash allocation; its storage must stay charged"
    );
    assert_eq!(cache.resident_charge(), before_resident - first_charge);
    assert_eq!(cache.link_owners[7], 0);
    for (owner, slot) in [(&second, 8), (&third, 9)] {
        assert!(owner.is_current(&memory));
        assert_eq!(
            cache
                .lookup_linked(&memory, owner.entrypoints()[0].0)
                .unwrap()
                .1,
            slot
        );
    }
    let replacement = cache.admit(&memory, family(&memory, &pcs[..56])).unwrap();
    assert_ne!(replacement.identity(), first.identity());
    assert_eq!(cache.trace_entries.capacity(), 224);
    assert_eq!(
        cache.directory_charge(),
        before_directory,
        "reusing DELETED buckets must not create a new post-admission charge"
    );
    cache.install_link(&replacement, 7).unwrap();
    for pc in &pcs[..56] {
        let (owner, slot) = cache.lookup_linked(&memory, *pc).unwrap();
        assert_eq!(owner.identity(), replacement.identity());
        assert_eq!(slot, 7);
    }
    assert_eq!(cache.snapshot()[2..5], [0, 0, 0]);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
}

#[test]
fn near_budget_family_growth_reuses_deleted_aliases_without_exceeding_residency() {
    let mut cache = CoreCache32::default();
    let (memory, pcs) = collision_memory(&cache);
    let first = cache.admit(&memory, family(&memory, &pcs[..56])).unwrap();
    let second = cache
        .admit(&memory, family(&memory, &pcs[56..112]))
        .unwrap();
    let third = cache.admit(&memory, family(&memory, &pcs[112..])).unwrap();
    cache.install_link(&first, 7).unwrap();
    cache.install_link(&second, 8).unwrap();
    cache.install_link(&third, 9).unwrap();

    // This is executable growth of the same real family: the first indirect
    // jump has now actually returned to its own PC as well as its old target.
    let growth = [(pcs[0], pcs[0])];
    let mut replacement = emit_observed_plan(&memory, plan(&memory, &growth, &[&first]))
        .unwrap()
        .unwrap();
    assert!(replacement.observed_grows(&first));
    let replacement_bytes = replacement.wasm().to_vec();
    replacement.reserve_wasm_capacity_for_test(64 << 10);
    assert_eq!(replacement.wasm(), replacement_bytes);
    let replacement_charge =
        CoreCache32::code_charge(&replacement, replacement.entrypoints().len());
    let first_charge = cache.region(first.key).unwrap().charge;

    // Charge real reserved artifact storage, rather than forging a counter or
    // lowering the production budget. The padding leaves a small exact margin
    // for the valid larger family after its parent storage has been released.
    let mut filler = crate::cpu::core::compile(
        &memory,
        GuestAddress(0x50000),
        CompileOptions {
            instructions: 2,
            blocks: 1,
            ..CompileOptions::default()
        },
    )
    .unwrap();
    let filler_bytes = filler.wasm().to_vec();
    let projected = cache.projected_directory(
        filler.entrypoints().len(),
        filler.instruction_count(),
        false,
    );
    let fixed = cache.owner_charge - first_charge
        + replacement_charge
        + projected
        + CoreCache32::code_charge(&filler, filler.entrypoints().len());
    let extra = MAX_CODE_BYTES.checked_sub(fixed + (16 << 10)).unwrap();
    filler.reserve_wasm_capacity_for_test(extra);
    assert_eq!(filler.wasm(), filler_bytes);
    let filler = cache.admit(&memory, filler).unwrap();
    cache.install_link(&filler, 10).unwrap();
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    assert!(MAX_CODE_BYTES - cache.resident_charge() < 1 << 20);

    let admission = cache
        .preflight_observed(&memory, &plan(&memory, &growth, &[&first]))
        .unwrap();
    let before = cache.snapshot();
    let grown = cache
        .commit_observed(&memory, admission, replacement)
        .unwrap();
    assert_eq!(cache.snapshot()[0], before[0] + 1);
    assert_eq!(cache.snapshot()[2..5], [0, 0, 0]);
    assert_eq!(cache.snapshot()[5], before[5]);
    assert_eq!(cache.oldest, Some(second.key.index));
    assert_eq!(cache.newest, Some(grown.key.index));
    assert!(cache.retained(&first).is_none());
    assert_eq!(cache.link_owners[7], 0);
    cache.install_link(&grown, 7).unwrap();
    for (owner, slot) in [(&grown, 7), (&second, 8), (&third, 9), (&filler, 10)] {
        assert!(owner.is_current(&memory));
        assert!(cache.retained(owner).is_some());
        assert_eq!(cache.link_owners[slot as usize], owner.identity());
        assert_eq!(cache.slot_regions[slot as usize] as usize, owner.key.index);
        for pc in owner.entrypoints() {
            let (selected, selected_slot) = cache.lookup_linked(&memory, pc.0).unwrap();
            assert_eq!(selected.identity(), owner.identity());
            assert_eq!(selected_slot, slot);
        }
    }
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    assert!(MAX_CODE_BYTES - cache.resident_charge() < 128 << 10);
    assert_eq!(
        cache.owner_charge,
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.charge)
            .sum::<usize>()
    );
    assert_eq!(cache.snapshot()[8], cache.resident_charge() as u64);
    assert_eq!(cache.snapshot()[9], MAX_CODE_BYTES as u64);
}
