use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, WORK_CONSUMED_OFFSET, X87_OFFSET},
        header,
        x86::{encode_exit_v7, encode_state_v3},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, ExecutionConfig32, HostError, calls::call::PendingOwner},
};

const KEY: u64 = 97;

fn fixture() -> (EngineInstance, [ResidentInstallation; 2]) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for pc in [0x1000, 0x1020] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), &[0x90, 0xc3])
            .unwrap();
    }
    engine
        .configure_execution(ExecutionConfig32 {
            units: 4,
            wasm_bytes: 262144,
            promotion_hits: 8,
        })
        .unwrap();
    let installed = [0x1000, 0x1020].map(|pc| {
        let row = engine.prepare_cache_entry(KEY, pc).unwrap();
        engine
            .install_cached_entry(KEY, row.serial, row.unit_id, row.slot)
            .unwrap()
    });
    (engine, installed)
}

fn stop(engine: &mut EngineInstance, reason: ExitReason, retired: u32) {
    let mut state = State32 {
        eip: 0x1020,
        eflags: 2,
        ..State32::default()
    };
    state.registers[4] = 0x8800;
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[..crate::abi::x86::STATE_SIZE]).unwrap();
    encode_exit_v7(
        &ExecutionExit { reason, retired },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE],
    )
    .unwrap();
    header::write_u32(arena, WORK_CONSUMED_OFFSET, retired);
}

fn returned(engine: &mut EngineInstance, installed: ResidentInstallation) -> u64 {
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(installed.unit_id))
        .unwrap();
    stop(engine, ExitReason::NeedCode, 2);
    engine
        .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    epoch
}

#[test]
fn dispatcher_entry_expectation_rejects_swapped_owners_before_issuing_epoch() {
    let (mut engine, installed) = fixture();
    let frame = engine
        .begin_execution_receipt(KEY, 10, installed[0].unit_id, true)
        .unwrap();
    engine.expect_execution_lookup(Some(installed[0]));
    let epoch = engine.detection_epoch;
    assert_eq!(
        engine.begin_exception_detection(PendingOwner::Resident(installed[1].unit_id)),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.detection_epoch, epoch);
    assert_eq!(engine.execution_frame.unwrap().expected, Some(installed[0]));
    let epoch = returned(&mut engine, installed[0]);
    assert_eq!(
        engine.execution_frame.unwrap().last,
        Some((epoch, installed[0]))
    );
    assert_eq!(engine.execution_frame.unwrap().expected, None);
    assert_eq!(
        engine.begin_exception_detection(PendingOwner::Resident(installed[0].unit_id)),
        Err(HostError::InvalidArtifact)
    );
    header::write_u32(engine.arena_mut().unwrap(), EXIT_OFFSET + 20, 2);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!(
        (
            receipt.epoch,
            receipt.owner,
            receipt.slot,
            receipt.total_retired,
            receipt.child_retired,
            receipt.kind
        ),
        (epoch, installed[0].unit_id, installed[0].slot, 2, 2, 2)
    );
    assert_eq!(header::read_u32(engine.arena(), EXIT_OFFSET + 20), 2);
    assert_eq!(
        engine.validate_execution_return_epoch(epoch),
        Ok(PendingOwner::Resident(installed[0].unit_id))
    );
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::Call(CallError::InvalidToken))
    );
}

#[test]
fn terminal_rejects_tampering_wrong_tokens_and_slot_changes_without_restoring_or_consuming() {
    let (mut engine, installed) = fixture();
    let frame = engine
        .begin_execution_receipt(KEY, 10, installed[0].unit_id, false)
        .unwrap();
    returned(&mut engine, installed[0]);
    header::write_u32(engine.arena_mut().unwrap(), EXIT_OFFSET + 20, 2);
    let valid = engine.arena().to_vec();
    for (key, token) in [(KEY ^ 1, frame.token), (KEY, 0), (KEY, frame.token + 1)] {
        assert!(engine.finish_execution_receipt(key, token).is_err());
        assert_eq!(engine.arena(), valid);
        assert_eq!(engine.execution_frame.unwrap().token, frame.token);
    }
    for offset in [16, X87_OFFSET, WORK_CONSUMED_OFFSET, EXIT_OFFSET + 24] {
        engine.arena_mut().unwrap()[offset] ^= 1;
        let changed = engine.arena().to_vec();
        assert!(engine.finish_execution_receipt(KEY, frame.token).is_err());
        assert_eq!(engine.arena(), changed);
        assert_eq!(engine.execution_frame.unwrap().token, frame.token);
        engine.arena_mut().unwrap().copy_from_slice(&valid);
    }
    for retired in [1, 11] {
        header::write_u32(engine.arena_mut().unwrap(), EXIT_OFFSET + 20, retired);
        let changed = engine.arena().to_vec();
        assert_eq!(
            engine.finish_execution_receipt(KEY, frame.token),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert_eq!(engine.arena(), changed);
    }
    engine.arena_mut().unwrap().copy_from_slice(&valid);
    engine.resident_installations[installed[0].slot as usize] = None;
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.arena(), valid);
    engine.resident_installations[installed[0].slot as usize] = Some(installed[0]);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
}

#[test]
fn direct_active_stop_and_no_child_dispatch_stops_keep_distinct_receipts() {
    for (reason, budget, cancel) in [(ExitReason::Budget, 0, 0), (ExitReason::Cancelled, 10, 1)] {
        let (mut engine, installed) = fixture();
        let frame = engine
            .begin_execution_receipt(KEY, budget, installed[0].unit_id, true)
            .unwrap();
        stop(&mut engine, reason, 0);
        header::write_u32(
            engine.arena_mut().unwrap(),
            crate::abi::arena::CANCEL_OFFSET,
            cancel,
        );
        let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
        assert_eq!(
            (receipt.kind, receipt.epoch, receipt.owner, receipt.slot),
            (0, 0, 0, u32::MAX)
        );
    }
    let (mut engine, installed) = fixture();
    let frame = engine
        .begin_execution_receipt(KEY, 10, installed[0].unit_id, false)
        .unwrap();
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(installed[0].unit_id))
        .unwrap();
    stop(&mut engine, ExitReason::Budget, 10);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!(
        (
            receipt.kind,
            receipt.epoch,
            receipt.total_retired,
            receipt.child_retired
        ),
        (1, epoch, 10, 10)
    );
}

#[test]
fn begin_failures_keep_frame_serial_and_ready_detection_authority() {
    let (mut engine, installed) = fixture();
    for (key, unit) in [(KEY ^ 1, installed[0].unit_id), (KEY, u64::MAX)] {
        assert!(
            engine
                .begin_execution_receipt(key, 10, unit, false)
                .is_err()
        );
        assert!(engine.execution_frame.is_none());
        assert_eq!(engine.execution_frame_issuer, 0);
    }
    engine.execution_frame_issuer = u64::MAX;
    assert_eq!(
        engine.begin_execution_receipt(KEY, 10, installed[0].unit_id, false),
        Err(HostError::GenerationExhausted)
    );
    assert!(engine.execution_frame.is_none());
    engine.execution_frame_issuer = 0;
    let frame = engine
        .begin_execution_receipt(KEY, 10, installed[0].unit_id, false)
        .unwrap();
    assert_eq!(
        engine.begin_execution_receipt(KEY, 10, installed[0].unit_id, false),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.execution_frame.unwrap().token, frame.token);
    assert!(engine.cache_pins().unwrap().contains(&installed[0].unit_id));
    engine.close();
    assert!(engine.execution_frame.is_none());
}

#[test]
fn synthetic_dispatch_stops_validate_cause_and_restore_the_full_child_seal() {
    use crate::abi::x86::encode_exit_v3;
    for reason in [
        ExitReason::NeedCode,
        ExitReason::Budget,
        ExitReason::Cancelled,
    ] {
        let (mut engine, installed) = fixture();
        let frame = engine
            .begin_execution_receipt(KEY, 2, installed[0].unit_id, true)
            .unwrap();
        engine.expect_execution_lookup(Some(installed[0]));
        let epoch = returned(&mut engine, installed[0]);
        let seal = engine.arena()[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE].to_vec();
        encode_exit_v3(
            &ExecutionExit { reason, retired: 2 },
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE],
        )
        .unwrap();
        if reason == ExitReason::Cancelled {
            let before = engine.arena().to_vec();
            assert!(engine.finish_execution_receipt(KEY, frame.token).is_err());
            assert_eq!(engine.arena(), before);
            header::write_u32(
                engine.arena_mut().unwrap(),
                crate::abi::arena::CANCEL_OFFSET,
                1,
            );
        }
        let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
        assert_eq!(
            receipt.reason,
            match reason {
                ExitReason::Budget => 1,
                ExitReason::Cancelled => 2,
                _ => 3,
            }
        );
        assert_eq!(receipt.total_retired, 2);
        assert_eq!(
            &engine.arena()[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE],
            seal
        );
        assert_eq!(
            engine.validate_execution_return_epoch(epoch),
            Ok(PendingOwner::Resident(installed[0].unit_id))
        );
    }
    let (mut engine, installed) = fixture();
    let frame = engine
        .begin_execution_receipt(KEY, 3, installed[0].unit_id, true)
        .unwrap();
    engine.expect_execution_lookup(Some(installed[0]));
    returned(&mut engine, installed[0]);
    encode_exit_v3(
        &ExecutionExit {
            reason: ExitReason::Budget,
            retired: 2,
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + crate::abi::x86::EXIT_SIZE],
    )
    .unwrap();
    assert!(engine.finish_execution_receipt(KEY, frame.token).is_err());
}

#[test]
fn generated_fault_event_and_partial_rep_keep_sealed_context_after_outer_folding() {
    for (reason, fields, retired, work) in [
        (
            ExitReason::InvalidOpcode,
            [20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            0,
            0,
        ),
        (
            ExitReason::DebugTrap,
            [16, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0],
            0,
            1,
        ),
    ] {
        let (mut engine, installed) = fixture();
        let frame = engine
            .begin_execution_receipt(KEY, 10, installed[0].unit_id, true)
            .unwrap();
        engine.expect_execution_lookup(Some(installed[0]));
        returned(&mut engine, installed[0]);
        engine.expect_execution_lookup(Some(installed[1]));
        let epoch = engine
            .begin_exception_detection(PendingOwner::Resident(installed[1].unit_id))
            .unwrap();
        stop(&mut engine, reason, retired);
        header::write_u32(engine.arena_mut().unwrap(), WORK_CONSUMED_OFFSET, work);
        engine.finish_generated_detection(epoch, fields).unwrap();
        assert!(engine.cpu_event.is_some());
        header::write_u32(engine.arena_mut().unwrap(), EXIT_OFFSET + 20, 2 + retired);
        let good = engine.arena().to_vec();
        engine.arena_mut().unwrap()[X87_OFFSET] ^= 1;
        assert!(engine.finish_execution_receipt(KEY, frame.token).is_err());
        engine.arena_mut().unwrap().copy_from_slice(&good);
        let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
        assert_eq!(
            (
                receipt.kind,
                receipt.owner,
                receipt.total_retired,
                receipt.child_retired
            ),
            (3, installed[1].unit_id, 2 + retired, retired)
        );
        assert_eq!(
            engine.validate_execution_return_epoch(epoch),
            Ok(PendingOwner::Resident(installed[1].unit_id))
        );
        assert_eq!(engine.cpu_event.as_ref().unwrap().work_consumed, work);
    }
}

#[test]
fn failed_child_sealing_does_not_advance_aggregate_and_active_terminal_counts_only_its_child() {
    let (mut engine, installed) = fixture();
    let frame = engine
        .begin_execution_receipt(KEY, 10, installed[0].unit_id, true)
        .unwrap();
    engine.expect_execution_lookup(Some(installed[0]));
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(installed[0].unit_id))
        .unwrap();
    stop(&mut engine, ExitReason::NeedCode, 2);
    assert!(
        engine
            .finish_generated_detection(epoch, [3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        (
            engine.execution_frame.unwrap().sealed_retired,
            engine.execution_frame.unwrap().sealed_work
        ),
        (0, 0)
    );
    engine
        .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(
        (
            engine.execution_frame.unwrap().sealed_retired,
            engine.execution_frame.unwrap().sealed_work
        ),
        (2, 2)
    );
    engine.expect_execution_lookup(Some(installed[1]));
    engine
        .begin_exception_detection(PendingOwner::Resident(installed[1].unit_id))
        .unwrap();
    stop(&mut engine, ExitReason::Budget, 5);
    header::write_u32(engine.arena_mut().unwrap(), WORK_CONSUMED_OFFSET, 3);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!(
        (receipt.total_retired, receipt.child_retired, receipt.kind),
        (5, 3, 1)
    );
    assert_eq!(header::read_u32(engine.arena(), EXIT_OFFSET + 20), 5);
}

#[test]
fn direct_entry_resets_previous_work_only_after_all_begin_validation() {
    let (mut engine, installed) = fixture();
    header::write_u32(engine.arena_mut().unwrap(), WORK_CONSUMED_OFFSET, 100);
    let before = engine.arena().to_vec();
    assert!(
        engine
            .begin_execution_receipt(KEY ^ 1, 1, installed[0].unit_id, false)
            .is_err()
    );
    assert_eq!(engine.arena(), before);
    let frame = engine
        .begin_execution_receipt(KEY, 1, installed[0].unit_id, false)
        .unwrap();
    assert_eq!(header::read_u32(engine.arena(), WORK_CONSUMED_OFFSET), 0);
    engine
        .begin_exception_detection(PendingOwner::Resident(installed[0].unit_id))
        .unwrap();
    stop(&mut engine, ExitReason::Budget, 1);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!(receipt.total_retired, 1);
}
