use super::admission::{KEY, install};
use crate::{
    abi::x86::{EXIT_SIZE, STATE_SIZE},
    cpu::{
        dbt::{CompilationTier32, RegistryError},
        x86::{SegmentRegister, State32},
    },
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, ExecutionConfig32, HostError,
        calls::call::{PendingCall, PendingOwner},
        cpu::context::ExtendedCpuState,
        execution::cache::{CacheRequest32, CacheRequestKind32},
    },
    windows::{CallFrame32, CallingConvention32},
};

fn fixture() -> (EngineInstance, [CacheRequest32; 4], CacheRequest32) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for (pc, code) in [
        (0x1000, &[0xb8, 1, 0, 0, 0, 0x85, 0xc9, 0x74, 0x17][..]),
        (0x1009, &[0x83, 0xc0, 2, 0xeb, 0x22][..]),
        (0x1020, &[0x83, 0xc0, 4, 0xeb, 0x0b][..]),
        (0x1030, &[0x01, 0xd8, 0xc3][..]),
        (0x1080, &[0x90, 0xc3][..]),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), code)
            .unwrap();
    }
    engine
        .configure_execution(ExecutionConfig32 {
            units: 8,
            wasm_bytes: 1048576,
            promotion_hits: 8,
        })
        .unwrap();
    // root is deliberately newer than its successors; plan order must not follow UnitId ordering.
    let left = install(&mut engine, 0x1009);
    let right = install(&mut engine, 0x1020);
    let tail = install(&mut engine, 0x1030);
    let root = install(&mut engine, 0x1000);
    let unrelated = install(&mut engine, 0x1080);
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    (engine, [root, left, right, tail], unrelated)
}

fn acknowledge(engine: &mut EngineInstance, request: CacheRequest32) {
    engine
        .acknowledge_cache_eviction(KEY, request.serial, request.victim_id, request.victim_slot)
        .unwrap();
}
fn pin(engine: &mut EngineInstance, id: u64) {
    engine.write32(0x8000, 0x1000).unwrap();
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        State32 {
            registers: [0, 0, 0, 0, 0x8000, 0, 0, 0],
            eip: 0x1009,
            eflags: 2,
        },
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(id),
        frame,
        state: [0; STATE_SIZE],
        exit: [0; EXIT_SIZE],
        extended: ExtendedCpuState::initial().unwrap(),
    });
}

#[test]
fn fully_warm_cohort_requires_serial_acknowledgements_then_preserves_every_side_entry() {
    let (mut engine, owners, unrelated) = fixture();
    let unrelated_bytes = engine.resident_bytes(unrelated.unit_id).unwrap().to_vec();
    let mut request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    for owner in owners {
        assert_eq!(request.kind, CacheRequestKind32::Evict);
        assert_eq!(
            (request.victim_id, request.victim_slot),
            (owner.unit_id, owner.slot)
        );
        assert_eq!(
            engine.prepare_cache_entry(KEY, 0x1000),
            Err(HostError::Call(CallError::Busy))
        );
        let serial = request.serial;
        acknowledge(&mut engine, request);
        assert!(
            engine
                .acknowledge_cache_eviction(KEY, serial, owner.unit_id, owner.slot)
                .is_err()
        );
        request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
        assert!(request.serial > serial);
    }
    assert_eq!(request.kind, CacheRequestKind32::Install);
    assert_eq!(request.tier, CompilationTier32::Optimized);
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    let region = engine
        .resident
        .as_ref()
        .unwrap()
        .get_raw(engine.memory.as_ref().unwrap(), request.unit_id)
        .unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (4, 9)
    );
    for pc in [
        0x1000, 0x1005, 0x1007, 0x1009, 0x100c, 0x1020, 0x1023, 0x1030, 0x1032,
    ] {
        assert_eq!(
            engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
            request.unit_id
        );
    }
    assert_eq!(engine.cache_usage().unwrap().evictions, 4);
    assert_eq!(engine.cache_usage().unwrap().promotions, 1);
    assert_eq!(engine.cache_usage().unwrap().installed, 2);
    assert_eq!(
        engine.resident_bytes(unrelated.unit_id).unwrap(),
        unrelated_bytes
    );
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1080)
            .unwrap()
            .unit_id,
        unrelated.unit_id
    );
}

#[test]
fn partial_cohort_cancellation_preserves_unacknowledged_owners_and_rebuilds_missing_root_cold() {
    let (mut engine, owners, _) = fixture();
    let first = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    acknowledge(&mut engine, first);
    let next = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    engine.cancel_cache_request(KEY, next.serial).unwrap();
    assert!(engine.execution_cache.as_ref().unwrap().cohort.is_none());
    assert_eq!(
        engine.lookup_installed_resident(KEY, 0x1000),
        Err(HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(0x1000)
        }))
    );
    for owner in &owners[1..] {
        assert_eq!(
            engine
                .resident_installation_by_id(owner.unit_id)
                .unwrap()
                .slot,
            owner.slot
        );
    }
    let rebuilt = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    assert_eq!(rebuilt.tier, CompilationTier32::Baseline);
    engine
        .install_cached_entry(KEY, rebuilt.serial, rebuilt.unit_id, rebuilt.slot)
        .unwrap();
    assert_ne!(rebuilt.unit_id, owners[0].unit_id);
    assert_eq!(engine.cache_usage().unwrap().evictions, 1);
}

#[test]
fn each_cohort_ack_rechecks_new_pins_exact_slots_and_code_version_before_removal() {
    for changed in 0..3 {
        let (mut engine, owners, _) = fixture();
        let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
        let before = engine.cache_usage().unwrap();
        match changed {
            0 => pin(&mut engine, owners[1].unit_id),
            1 => engine.resident_installations[owners[1].slot as usize] = None,
            _ => engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(0x1ff0), &[1])
                .unwrap(),
        }
        assert!(
            engine
                .acknowledge_cache_eviction(
                    KEY,
                    request.serial,
                    request.victim_id,
                    request.victim_slot
                )
                .is_err()
        );
        assert_eq!(engine.cache_usage().unwrap(), before);
        assert!(
            engine
                .resident
                .as_ref()
                .unwrap()
                .get_retained_raw(engine.memory.as_ref().unwrap(), owners[0].unit_id)
                .is_ok()
        );
        engine.cancel_cache_request(KEY, request.serial).unwrap();
        assert!(engine.execution_cache.as_ref().unwrap().cohort.is_none());
    }
}

#[test]
fn changed_decode_context_and_different_next_pc_abandon_the_cohort_without_wedging_the_cache() {
    let (mut engine, owners, _) = fixture();
    let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    acknowledge(&mut engine, request);
    let unrelated = engine.prepare_cache_entry(KEY, 0x1080).unwrap();
    assert_eq!(unrelated.kind, CacheRequestKind32::Hit);
    assert!(engine.execution_cache.as_ref().unwrap().cohort.is_none());
    assert_eq!(
        engine
            .resident_installation_by_id(owners[1].unit_id)
            .unwrap()
            .slot,
        owners[1].slot
    );

    let (mut engine, owners, _) = fixture();
    let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    acknowledge(&mut engine, request);
    engine
        .set_segment_base(SegmentRegister::Cs, 0x4000)
        .unwrap();
    assert_eq!(
        engine.prepare_cache_entry(KEY, 0x1000),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert!(engine.execution_cache.as_ref().unwrap().cohort.is_none());
    assert_eq!(
        engine
            .resident_installation_by_id(owners[1].unit_id)
            .unwrap()
            .slot,
        owners[1].slot
    );
    // retrying the current context is an ordinary cold request, not a permanently busy plan.
    assert_ne!(
        engine.prepare_cache_entry(KEY, 0x1000),
        Err(HostError::Call(CallError::Busy))
    );
}

#[test]
fn explicit_cancel_in_the_ack_gap_consumes_only_the_matching_cohort_token() {
    let (mut engine, owners, _) = fixture();
    let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    acknowledge(&mut engine, request);
    assert!(
        engine
            .cancel_cache_request(KEY, request.serial + 1)
            .is_err()
    );
    assert!(engine.execution_cache.as_ref().unwrap().cohort.is_some());
    engine.cancel_cache_request(KEY, request.serial).unwrap();
    assert!(engine.cancel_cache_request(KEY, request.serial).is_err());
    assert_eq!(
        engine
            .resident_installation_by_id(owners[1].unit_id)
            .unwrap()
            .slot,
        owners[1].slot
    );
    assert_eq!(
        engine.prepare_cache_entry(KEY, 0x1000).unwrap().tier,
        CompilationTier32::Baseline
    );
}

#[test]
fn a_pinned_successor_remains_a_join_instead_of_becoming_a_retirement_victim() {
    let (mut engine, owners, _) = fixture();
    pin(&mut engine, owners[1].unit_id);
    let mut request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    for _ in 0..8 {
        assert_ne!(request.victim_id, owners[1].unit_id);
        if request.kind == CacheRequestKind32::Install {
            break;
        }
        acknowledge(&mut engine, request);
        request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    }
    assert_eq!(request.kind, CacheRequestKind32::Install);
    assert_eq!(
        engine
            .resident_installation_by_id(owners[1].unit_id)
            .unwrap()
            .slot,
        owners[1].slot
    );
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1009)
            .unwrap()
            .unit_id,
        owners[1].unit_id
    );
}

#[test]
fn publication_in_the_final_ack_gap_invalidates_only_the_unpublished_replacement() {
    let (mut engine, owners, _) = fixture();
    let mut request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    for (index, _) in owners.iter().enumerate() {
        acknowledge(&mut engine, request);
        if index + 1 < owners.len() {
            request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
        }
    }
    let arena = engine.arena_mut().unwrap();
    arena[140..148].copy_from_slice(&[0, 0x10, 0, 0, 0, 0, 0, 0]);
    let existing = engine.compile_resident_entries(1, 0).unwrap().get();
    let bytes = engine.resident_bytes(existing).unwrap().to_vec();
    assert_eq!(
        engine.prepare_cache_entry(KEY, 0x1000),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert!(
        engine
            .execution_cache
            .as_ref()
            .unwrap()
            .replacement
            .is_none()
    );
    assert_eq!(engine.resident_bytes(existing).unwrap(), bytes);
    let install = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    assert_eq!(install.unit_id, existing);
    assert_eq!(install.tier, CompilationTier32::Baseline);
}

pub(super) mod transaction;
