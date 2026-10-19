use super::fixture::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, WORK_CONSUMED_OFFSET},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v7, encode_state_v3},
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::CompilationTier32,
        x86::{SegmentRegister, State32},
    },
    memory::GuestAddress,
    process::{
        CallError, HostError, ResidentInstallation,
        calls::call::PendingOwner,
        execution::cache::{CacheRequest32, CacheRequestKind32},
    },
};

fn train(fixture: &mut Fixture) {
    // native detector protocol control, not a claim that this test executes guest bytes.
    // only the real validated reason-3 finish seam trains the production cache row.
    for (index, repeats) in [9, 9, 8].into_iter().enumerate() {
        let owner = fixture.owners[index];
        for _ in 0..repeats {
            let engine = &mut fixture.engine;
            let arena = engine.arena.as_mut().get_mut();
            encode_state_v3(
                &State32 {
                    eip: owner.pc,
                    eflags: 2,
                    ..State32::default()
                },
                &mut arena[..STATE_SIZE],
            )
            .unwrap();
            let frame = engine
                .begin_execution_receipt(KEY, 2, owner.unit_id, true)
                .unwrap();
            engine.expect_execution_lookup(Some(ResidentInstallation {
                unit_id: owner.unit_id,
                slot: owner.slot,
            }));
            let epoch = engine
                .begin_exception_detection(PendingOwner::Resident(owner.unit_id))
                .unwrap();
            let arena = engine.arena.as_mut().get_mut();
            encode_state_v3(
                &State32 {
                    eip: TARGETS[index].unwrap(),
                    eflags: 2,
                    ..State32::default()
                },
                &mut arena[..STATE_SIZE],
            )
            .unwrap();
            encode_exit_v7(
                &ExecutionExit {
                    reason: ExitReason::NeedCode,
                    retired: 1,
                },
                &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
            )
            .unwrap();
            header::write_u32(arena, WORK_CONSUMED_OFFSET, 1);
            engine
                .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap();
            let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
            assert_eq!(
                (receipt.owner, receipt.reason, receipt.total_retired),
                (owner.unit_id, 3, 1)
            );
        }
    }
    for _ in 0..64 {
        fixture
            .engine
            .lookup_installed_resident(KEY, ROOTS[0])
            .unwrap();
    }
}

fn request(fixture: &mut Fixture) -> CacheRequest32 {
    train(fixture);
    let request = fixture
        .engine
        .prepare_cache_promotion(KEY)
        .unwrap()
        .expect("mature validated indirect hints make a genuine Tier1 root eligible");
    assert_eq!(
        (request.kind, request.victim_id),
        (CacheRequestKind32::Evict, fixture.owners[0].unit_id)
    );
    request
}

#[test]
fn automatic_hinted_promotion_uses_serial_ack_then_preserves_every_cold_and_interior_start() {
    let mut fixture = fixture();
    let before = retained(&fixture);
    let before_usage = fixture.engine.cache_usage().unwrap();
    let mut next = request(&mut fixture);
    assert_eq!(retained(&fixture), before);
    let mut last_serial = 0;
    for owner in fixture.owners {
        assert_eq!(
            (next.kind, next.victim_id, next.victim_slot),
            (CacheRequestKind32::Evict, owner.unit_id, owner.slot)
        );
        assert!(next.serial > last_serial);
        last_serial = next.serial;
        fixture
            .engine
            .acknowledge_cache_eviction(KEY, next.serial, next.victim_id, next.victim_slot)
            .unwrap();
        next = fixture.engine.prepare_cache_entry(KEY, ROOTS[0]).unwrap();
    }
    assert_eq!(
        (next.kind, next.tier),
        (CacheRequestKind32::Install, CompilationTier32::Optimized)
    );
    fixture
        .engine
        .install_cached_entry(KEY, next.serial, next.unit_id, next.slot)
        .unwrap();
    let region = fixture
        .engine
        .resident
        .as_ref()
        .unwrap()
        .get_raw(fixture.engine.memory.as_ref().unwrap(), next.unit_id)
        .unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (7, 17)
    );
    for pc in STARTS.into_iter().flatten() {
        assert!(region.contains_instruction(*pc));
        assert!(region.contains_executable(*pc));
        assert_eq!(
            fixture
                .engine
                .lookup_installed_resident(KEY, *pc)
                .unwrap()
                .unit_id,
            next.unit_id
        );
    }
    let usage = fixture.engine.cache_usage().unwrap();
    assert_eq!(usage.evictions, before_usage.evictions + 3);
    assert_eq!(usage.promotions, before_usage.promotions + 1);
    assert_eq!(usage.installed, 1);
}

#[test]
fn automatic_hint_replacement_wrong_ack_and_cancel_leave_old_owners_untouched() {
    let mut fixture = fixture();
    let next = request(&mut fixture);
    let usage = fixture.engine.cache_usage().unwrap();
    let before = retained(&fixture);
    let installations = fixture.engine.resident_installations_by_id.clone();
    assert_eq!(
        fixture.engine.acknowledge_cache_eviction(
            KEY,
            next.serial,
            fixture.owners[1].unit_id,
            next.victim_slot
        ),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        fixture.engine.acknowledge_cache_eviction(
            KEY,
            next.serial + 1,
            next.victim_id,
            next.victim_slot
        ),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(fixture.engine.cache_usage().unwrap(), usage);
    assert_eq!(fixture.engine.resident_installations_by_id, installations);
    assert_eq!(retained(&fixture), before);
    fixture
        .engine
        .cancel_cache_request(KEY, next.serial)
        .unwrap();
    assert_eq!(retained(&fixture), before);
    assert_old_lookup(&fixture);
}

#[test]
fn hinted_eviction_revalidates_source_decode_and_late_pins_before_any_removal() {
    for changed in 0..3 {
        let mut fixture = fixture();
        let next = request(&mut fixture);
        let before = retained(&fixture);
        let usage = fixture.engine.cache_usage().unwrap();
        let installations = fixture.engine.resident_installations_by_id.clone();
        let expected = match changed {
            0 => {
                fixture
                    .engine
                    .memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(0x2ff0), &[1])
                    .unwrap();
                HostError::Call(CallError::StateChanged)
            }
            1 => {
                fixture
                    .engine
                    .set_segment_base(SegmentRegister::Cs, 0x4000)
                    .unwrap();
                HostError::Call(CallError::StateChanged)
            }
            _ => {
                let b = fixture.owners[1];
                pin(&mut fixture.engine, b);
                HostError::Call(CallError::Busy)
            }
        };
        assert_eq!(
            fixture.engine.acknowledge_cache_eviction(
                KEY,
                next.serial,
                next.victim_id,
                next.victim_slot
            ),
            Err(expected)
        );
        assert_eq!(fixture.engine.cache_usage().unwrap(), usage);
        assert_eq!(fixture.engine.resident_installations_by_id, installations);
        assert_eq!(retained(&fixture), before);
    }
}
