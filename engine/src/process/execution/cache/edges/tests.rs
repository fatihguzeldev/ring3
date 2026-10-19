use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, WORK_CONSUMED_OFFSET},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v7, encode_state_v3},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{
        CallError, ExecutionConfig32, HostError, ResidentInstallation, calls::call::PendingOwner,
        execution::cache::CacheRequestKind32,
    },
};

const KEY: u64 = 0x4544_4745;

fn fixture() -> (EngineInstance, [ResidentInstallation; 2]) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for pc in [0x1000, 0x1020] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), &[0x90, 0x90, 0x90, 0xc3])
            .unwrap();
    }
    engine
        .configure_execution(ExecutionConfig32 {
            units: 4,
            wasm_bytes: 262144,
            promotion_hits: 2,
        })
        .unwrap();
    let mut units = [0x1000, 0x1020].map(|pc| {
        let request = engine.prepare_cache_entry(KEY, pc).unwrap();
        engine
            .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
            .unwrap()
    });
    for _ in 0..2 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    let eviction = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    assert_eq!(eviction.kind, CacheRequestKind32::Evict);
    assert_eq!(eviction.victim_id, units[0].unit_id);
    engine
        .acknowledge_cache_eviction(
            KEY,
            eviction.serial,
            eviction.victim_id,
            eviction.victim_slot,
        )
        .unwrap();
    let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    assert_eq!(request.tier, CompilationTier32::Optimized);
    units[0] = engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    (engine, units)
}

fn hint(engine: &EngineInstance, unit: ResidentInstallation) -> NeedCodeHint32 {
    engine.execution_cache.as_ref().unwrap().entries[unit.slot as usize]
        .as_ref()
        .unwrap()
        .need_code_hint
}

fn enter(engine: &mut EngineInstance, unit: ResidentInstallation) -> (u64, u64) {
    let frame = engine
        .begin_execution_receipt(KEY, 4, unit.unit_id, true)
        .unwrap();
    engine.expect_execution_lookup(Some(unit));
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(unit.unit_id))
        .unwrap();
    (frame.token, epoch)
}

fn output(engine: &mut EngineInstance, target: u32, reason: ExitReason, retired: u32) {
    let arena = engine.arena.as_mut().get_mut();
    encode_state_v3(
        &State32 {
            eip: target,
            eflags: 2,
            ..State32::default()
        },
        &mut arena[..STATE_SIZE],
    )
    .unwrap();
    encode_exit_v7(
        &ExecutionExit { reason, retired },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    header::write_u32(arena, WORK_CONSUMED_OFFSET, retired);
}

#[test]
fn hint_is_eight_bytes_and_zero_is_a_valid_target() {
    assert_eq!(std::mem::size_of::<NeedCodeHint32>(), 8);
    let mut hint = NeedCodeHint32::default();
    assert_eq!(hint.target(1), None);
    hint.observe(0);
    assert_eq!(hint.target(1), Some(0));
    assert_eq!(hint.target(2), None);
}

#[test]
fn competing_targets_cancel_votes_before_replacing_the_candidate() {
    let mut hint = NeedCodeHint32::default();
    hint.observe(0x1000);
    hint.observe(0x1000);
    hint.observe(0x2000);
    assert_eq!(hint.target(1), Some(0x1000));
    assert_eq!(hint.target(2), None);
    hint.observe(0x2000);
    assert_eq!(hint.target(1), None);
    hint.observe(0x2000);
    assert_eq!(hint.target(1), Some(0x2000));
}

#[test]
fn matching_votes_saturate_and_later_dissent_still_decrements() {
    let mut hint = NeedCodeHint32 {
        target: 42,
        score: u32::MAX,
    };
    hint.observe(42);
    assert_eq!(hint.score, u32::MAX);
    hint.observe(43);
    assert_eq!(hint.score, u32::MAX - 1);
}

#[test]
fn only_successfully_committed_need_code_seals_train_the_optimized_row() {
    let (mut engine, units) = fixture();
    let (_, epoch) = enter(&mut engine, units[0]);
    output(&mut engine, 0, ExitReason::NeedCode, 1);
    assert_eq!(
        engine.finish_generated_detection(epoch, [3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(hint(&engine, units[0]).score, 0);
    assert_eq!(
        engine.finish_generated_detection(epoch + 1, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(hint(&engine, units[0]).score, 0);
    engine
        .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(hint(&engine, units[0]).target(1), Some(0));
    assert_eq!(hint(&engine, units[0]).score, 1);
    assert!(
        engine
            .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .is_err()
    );
    assert_eq!(hint(&engine, units[0]).score, 1);
}

#[test]
fn absent_or_mismatched_child_and_installation_identity_is_neutral() {
    let (mut engine, units) = fixture();
    let (_, epoch) = enter(&mut engine, units[0]);
    output(&mut engine, 0x2000, ExitReason::NeedCode, 1);
    let arena = engine.arena().to_vec();
    engine.record_cache_need_code(epoch + 1);
    assert_eq!(hint(&engine, units[0]).score, 0);
    let canonical = engine.resident_installations[units[0].slot as usize].take();
    engine.record_cache_need_code(epoch);
    assert_eq!(hint(&engine, units[0]).score, 0);
    engine.resident_installations[units[0].slot as usize] = canonical;
    let cache = engine.execution_cache.as_mut().unwrap();
    cache.entries[units[0].slot as usize].as_mut().unwrap().id = units[1].unit_id;
    engine.record_cache_need_code(epoch);
    assert_eq!(hint(&engine, units[0]).score, 0);
    engine.execution_cache.as_mut().unwrap().entries[units[0].slot as usize]
        .as_mut()
        .unwrap()
        .id = units[0].unit_id;
    engine.execution_frame = None;
    engine.record_cache_need_code(epoch);
    assert_eq!(hint(&engine, units[0]).score, 0);
    assert_eq!(engine.arena(), arena);
}

#[test]
fn baseline_events_and_outer_budget_do_not_train_need_code_hints() {
    let (mut engine, units) = fixture();
    let (_, epoch) = enter(&mut engine, units[1]);
    output(&mut engine, 0x1000, ExitReason::NeedCode, 1);
    engine
        .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(hint(&engine, units[1]).score, 0);

    let (mut engine, units) = fixture();
    let (_, epoch) = enter(&mut engine, units[0]);
    output(&mut engine, 0x1000, ExitReason::InvalidOpcode, 0);
    engine
        .finish_generated_detection(epoch, [20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(hint(&engine, units[0]).score, 0);

    let (mut engine, units) = fixture();
    let (token, _) = enter(&mut engine, units[0]);
    output(&mut engine, 0x1000, ExitReason::Budget, 4);
    engine.finish_execution_receipt(KEY, token).unwrap();
    assert_eq!(hint(&engine, units[0]).score, 0);
}

#[test]
fn installation_of_a_new_row_does_not_reuse_its_predecessors_hint() {
    let (mut engine, units) = fixture();
    let (token, epoch) = enter(&mut engine, units[0]);
    output(&mut engine, 0x1020, ExitReason::NeedCode, 1);
    engine
        .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    engine.finish_execution_receipt(KEY, token).unwrap();
    assert_eq!(hint(&engine, units[0]).score, 1);
    engine
        .discard_installed_resident(KEY, units[0].unit_id, units[0].slot)
        .unwrap();
    let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    let installed = engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    assert_ne!(installed.unit_id, units[0].unit_id);
    assert_eq!(hint(&engine, installed).score, 0);
}
