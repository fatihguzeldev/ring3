use crate::{
    abi::{
        arena::CANCEL_OFFSET,
        x86::{EXIT_SIZE, STATE_SIZE},
    },
    cpu::x86::State32,
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, HostError,
        calls::call::{PendingCall, PendingOwner},
        cpu::context::ExtendedCpuState,
        execution::cache::{CacheRequest32, CacheRequestKind32, ExecutionConfig32},
    },
    windows::{CallFrame32, CallingConvention32},
};
pub(super) const KEY: u64 = 97;
pub(super) fn fixture() -> EngineInstance {
    fixture_units(2)
}
fn fixture_units(units: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for entry in [0x1000, 0x1020, 0x1040, 0x1060] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(entry), &[0x90, 0xc3])
            .unwrap();
    }
    engine
        .configure_execution(ExecutionConfig32 {
            units,
            wasm_bytes: 131072,
            promotion_hits: 8,
        })
        .unwrap();
    engine
}
pub(super) fn install(engine: &mut EngineInstance, pc: u32) -> CacheRequest32 {
    let request = engine.prepare_cache_entry(KEY, pc).unwrap();
    assert_eq!(request.kind, CacheRequestKind32::Install);
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    request
}
#[test]
fn eviction_requires_exact_quiescent_receipt_and_only_then_reclaims_slot_and_artifact() {
    let mut engine = fixture();
    let first = install(&mut engine, 0x1000);
    let second = install(&mut engine, 0x1020);
    engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    let eviction = engine.prepare_cache_entry(KEY, 0x1040).unwrap();
    assert_eq!(
        (eviction.kind, eviction.victim_id),
        (CacheRequestKind32::Evict, second.unit_id)
    );
    let before = engine.cache_usage().unwrap();
    let installations = engine.resident_installations_by_id.clone();
    assert_eq!(
        engine.acknowledge_cache_eviction(
            KEY,
            eviction.serial,
            first.unit_id,
            eviction.victim_slot
        ),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.cache_usage().unwrap(), before);
    assert_eq!(engine.resident_installations_by_id, installations);
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(engine.memory.as_ref().unwrap(), second.unit_id)
            .is_ok()
    );
    engine
        .acknowledge_cache_eviction(
            KEY,
            eviction.serial,
            eviction.victim_id,
            eviction.victim_slot,
        )
        .unwrap();
    assert_eq!(engine.cache_usage().unwrap().evictions, 1);
    assert!(
        engine
            .resident_installations_by_id
            .iter()
            .all(|installed| installed.unit_id != second.unit_id)
    );
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(engine.memory.as_ref().unwrap(), second.unit_id)
            .is_err()
    );
    let third = install(&mut engine, 0x1040);
    assert_eq!(third.slot, second.slot);
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1040)
            .unwrap()
            .unit_id,
        third.unit_id
    );
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1000)
            .unwrap()
            .unit_id,
        first.unit_id
    );
}
#[test]
fn source_change_and_cancellation_never_publish_an_unvalidated_candidate() {
    let mut engine = fixture();
    let candidate = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x91])
        .unwrap();
    assert_eq!(
        engine.install_cached_entry(KEY, candidate.serial, candidate.unit_id, candidate.slot),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(engine.cache_usage().unwrap().installed, 0);
    engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    engine.cancel_cache_request(KEY, candidate.serial).unwrap();
    assert_eq!(engine.cache_usage().unwrap().units, 0);
    assert!(engine.pending_cache_request().unwrap().is_none());
    assert_eq!(
        engine.prepare_cache_entry(KEY, 0x1000),
        Err(HostError::Call(CallError::Cancelled))
    );
}
#[test]
fn parked_call_owner_is_pinned_and_all_pinned_cache_rejects_admission_without_mutation() {
    let mut engine = fixture();
    let first = install(&mut engine, 0x1000);
    let second = install(&mut engine, 0x1020);
    engine.write32(0x8000, 0x1000).unwrap();
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        State32 {
            registers: [0, 0, 0, 0, 0x8000, 0, 0, 0],
            eip: 0x1000,
            eflags: 2,
        },
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(first.unit_id),
        frame,
        state: [0; STATE_SIZE],
        exit: [0; EXIT_SIZE],
        extended: ExtendedCpuState::initial().unwrap(),
    });
    let request = engine.prepare_cache_entry(KEY, 0x1040).unwrap();
    assert_eq!(request.victim_id, second.unit_id);
    engine.cancel_cache_request(KEY, request.serial).unwrap();
    engine.pending_call.as_mut().unwrap().owner = PendingOwner::Resident(second.unit_id);
    let second_request = engine.prepare_cache_entry(KEY, 0x1040).unwrap();
    assert_eq!(second_request.victim_id, first.unit_id);
    engine
        .cancel_cache_request(KEY, second_request.serial)
        .unwrap();
    let mut single = fixture_units(1);
    let only = install(&mut single, 0x1000);
    let mut pending = engine.pending_call.take().unwrap();
    pending.owner = PendingOwner::Resident(only.unit_id);
    single.pending_call = Some(pending);
    let before = single.cache_usage().unwrap();
    assert_eq!(
        single.prepare_cache_entry(KEY, 0x1040),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(single.cache_usage().unwrap(), before);
}

#[test]
fn cache_prefix_joins_installed_tail_and_smc_recompiles_page_dependents() {
    let mut engine = fixture_units(4);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x08, 0xcd, 0x90, 0x90, 0xc3])
        .unwrap();
    let tail = install(&mut engine, 0x1002);
    let prefix = install(&mut engine, 0x1000);
    assert_ne!(prefix.unit_id, tail.unit_id);
    let region = engine
        .resident
        .as_ref()
        .unwrap()
        .get_raw(engine.memory.as_ref().unwrap(), prefix.unit_id)
        .unwrap();
    assert_eq!(region.metadata().instructions, 1);
    assert!(!region.contains_executable(0x1002));
    let joined = engine.prepare_cache_entry(KEY, 0x1002).unwrap();
    assert_eq!(
        (joined.kind, joined.unit_id, joined.slot),
        (CacheRequestKind32::Hit, tail.unit_id, tail.slot)
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1002), &[0x40])
        .unwrap();
    assert_eq!(
        engine.lookup_installed_resident(KEY, 0x1000),
        Err(HostError::Resident(
            crate::cpu::dbt::RegistryError::CodeInvalidated
        ))
    );
    let replacement = install(&mut engine, 0x1002);
    assert_ne!(replacement.unit_id, tail.unit_id);
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1002)
            .unwrap()
            .unit_id,
        replacement.unit_id
    );
    let new_prefix = install(&mut engine, 0x1000);
    assert_ne!(new_prefix.unit_id, prefix.unit_id);
    assert_eq!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_raw(engine.memory.as_ref().unwrap(), new_prefix.unit_id)
            .unwrap()
            .metadata()
            .instructions,
        1
    );
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1000)
            .unwrap()
            .unit_id,
        new_prefix.unit_id
    );
}
