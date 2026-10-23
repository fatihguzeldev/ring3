use super::*;
use crate::{
    cpu::{
        core::{
            CompileOptions, NativeInstruction, compile, emit_observed_plan, prepare_observed_plan,
            trace::{CompletedColdObservation, PendingColdObservation},
        },
        x86::decode::{BoundDecode, DecodeAuthority, DecodePolicy, decode_for_compilation},
    },
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x40, 0xff, 0xe6, 0x41, 0xff, 0xe7])
        .unwrap();
    memory
}

fn compiled(memory: &AddressSpace, pc: u32) -> CompiledCode {
    compile(memory, GuestAddress(pc), CompileOptions::default()).unwrap()
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
        panic!("the fixture instruction is supported")
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

#[test]
fn declared_residency_includes_reserved_directories_and_releases_owner_storage() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let empty = cache.snapshot();
    assert_eq!(&empty[..8], &[0, 0, 0, 0, 0, 0, 0, MAX_REGIONS as u64]);
    assert!(
        empty[8] > 0,
        "the unused initial directories are still owned storage"
    );
    let code = compiled(&memory, 0x1000);
    let charge = CoreCache32::code_charge(&code, code.entrypoints().len());
    let owner = cache.admit(&memory, code).unwrap();
    assert_eq!(cache.owner_charge, charge);
    assert_eq!(cache.resident_charge(), cache.directory_charge() + charge);
    assert!(cache.resident_charge() > owner.wasm().len());
    assert_eq!(
        cache.snapshot()[11],
        empty[11],
        "an unbound insertion does not revoke a binding"
    );
    let before = cache.snapshot();
    cache.remove(owner.key);
    assert_eq!(cache.owner_charge, 0);
    assert_eq!(cache.resident_charge(), cache.directory_charge());
    assert!(cache.snapshot()[8] < before[8]);
    assert!(
        cache.snapshot()[8] >= empty[8],
        "retained hash capacity remains charged"
    );
    assert!(cache.snapshot()[11] > before[11]);
    assert_eq!(cache.snapshot()[6], 0);
}

#[test]
fn reciprocal_owner_handles_remain_exact_above_u16_range() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    while cache.regions.len() <= usize::from(u16::MAX) + 1 {
        cache.grow_directories();
    }
    let high = usize::from(u16::MAX) + 1;
    let position = cache.free.iter().position(|index| *index == high).unwrap();
    let last = cache.free.len() - 1;
    cache.free.swap(position, last);
    let owner = cache.admit(&memory, compiled(&memory, 0x1000)).unwrap();
    assert_eq!(owner.key.index, high);
    let slot = high as u32 + 1;
    cache.install_link(&owner, slot).unwrap();
    assert_eq!(cache.slot_regions[slot as usize], high as u32);
    assert_eq!(cache.lookup_linked(&memory, 0x1000).unwrap().1, slot);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    cache.unlink(slot, owner.identity()).unwrap();
    assert_eq!(cache.slot_regions[slot as usize], NO_REGION);
    cache.install_link(&owner, slot).unwrap();
    cache.remove(owner.key);
    assert_eq!(cache.link_owners[slot as usize], 0);
    assert_eq!(cache.slot_regions[slot as usize], NO_REGION);
}

#[test]
fn pressure_compaction_preserves_live_high_slots_and_regrows_only_empty_holes() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let owner = cache.admit(&memory, compiled(&memory, 0x1000)).unwrap();
    cache.grow_directories();
    let high_slot = cache.link_capacity() as u32;
    cache.install_link(&owner, high_slot).unwrap();
    let bound = cache.snapshot();
    cache.compact_directories();
    assert_eq!(cache.link_capacity(), high_slot as usize);
    assert_eq!(cache.snapshot()[11], bound[11]);
    assert_eq!(cache.lookup_linked(&memory, 0x1000).unwrap().1, high_slot);
    cache.unlink(high_slot, owner.identity()).unwrap();
    let unlinked = cache.snapshot();
    cache.compact_directories();
    assert_eq!(cache.link_capacity(), MAX_LINK_SLOT);
    assert!(cache.snapshot()[11] > unlinked[11]);
    assert!(cache.retained(&owner).is_some());
    cache.grow_directories();
    assert_eq!(cache.link_owners[high_slot as usize], 0);
    assert_eq!(cache.slot_regions[high_slot as usize], NO_REGION);
    cache.install_link(&owner, high_slot).unwrap();
    assert_eq!(cache.lookup_linked(&memory, 0x1000).unwrap().1, high_slot);
}

#[test]
fn binding_epoch_tracks_revocations_and_stays_unconditional_after_wrap() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let owner = cache.admit(&memory, compiled(&memory, 0x1000)).unwrap();
    let before = cache.snapshot()[11];
    cache.install_link(&owner, 1).unwrap();
    assert!(cache.snapshot()[11] > before);
    let installed = cache.snapshot()[11];
    cache.install_link(&owner, 1).unwrap();
    assert_eq!(cache.snapshot()[11], installed);
    cache.unlink(2, owner.identity()).unwrap();
    assert_eq!(cache.snapshot()[11], installed);
    cache.binding_epoch = u64::MAX;
    cache.unlink(1, owner.identity()).unwrap();
    assert_eq!(cache.snapshot()[11], 0);
    cache.install_link(&owner, 1).unwrap();
    cache.grow_directories();
    cache.remove(owner.key);
    assert_eq!(cache.snapshot()[11], 0);
}

#[test]
fn reserved_artifact_capacity_over_budget_cannot_retire_a_valid_family_parent() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = emit_observed_plan(&memory, plan(&memory, &[(0x1000, 0x1001)], &[]))
        .unwrap()
        .unwrap();
    let owner = cache.admit(&memory, first).unwrap();
    cache.install_link(&owner, 7).unwrap();
    let next = plan(
        &memory,
        &[(0x1000, 0x1001), (0x1001, 0x1003), (0x1003, 0x1004)],
        &[&owner],
    );
    let admission = cache.preflight_observed(&memory, &next).unwrap();
    let mut next = emit_observed_plan(&memory, next).unwrap().unwrap();
    assert!(next.observed_grows(&owner));
    let encoded = next.wasm().to_vec();
    next.reserve_wasm_capacity_for_test(MAX_CODE_BYTES);
    assert_eq!(next.wasm(), encoded);
    assert!(next.resident_charge() > MAX_CODE_BYTES);
    let before = cache.snapshot();
    assert!(matches!(
        cache.commit_observed(&memory, admission, next),
        Err(CompileError::WasmLimit)
    ));
    assert_eq!(cache.snapshot(), before);
    let (selected, slot) = cache.lookup_linked(&memory, 0x1000).unwrap();
    assert_eq!(slot, 7);
    assert_eq!(selected.identity(), owner.identity());
    assert!(Rc::ptr_eq(&selected.code, &owner.code));
    assert_eq!(cache.link_owners[7], owner.identity());
    assert_eq!(cache.trace_entries.get(&0x1000), Some(&owner.key));
    assert!(!cache.trace_entries.contains_key(&0x1003));
}
