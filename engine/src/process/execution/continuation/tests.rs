use crate::{
    abi::{
        arena::{EXIT_OFFSET, WORK_CONSUMED_OFFSET, X87_OFFSET},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v7, encode_state_v3},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, ExecutionConfig32, HostError, calls::call::PendingOwner,
        execution::installation::ResidentInstallation,
    },
};

const KEY: u64 = 0x434f4e54;

fn fixture() -> (EngineInstance, [ResidentInstallation; 3]) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for (page, rights) in [(0x1000, 7), (0x2000, 7), (0x5000, 3), (0x8000, 3)] {
        engine.map(page, 1, rights).unwrap();
    }
    // ordinary native x87 + integer, a second eligible child, and strict xorps.
    for (pc, bytes) in [
        (0x1000, &[0xd9, 0xe8, 0xd9, 0xe0, 0x90, 0xc3][..]),
        (0x1020, &[0x90, 0xc3][..]),
        (0x2000, &[0x0f, 0x57, 0xc0, 0xc3][..]),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), bytes)
            .unwrap();
    }
    engine
        .configure_execution(ExecutionConfig32 {
            units: 8,
            wasm_bytes: 262144,
            promotion_hits: 256,
        })
        .unwrap();
    let units = [0x1000, 0x1020, 0x2000].map(|pc| {
        let row = engine.prepare_cache_entry(KEY, pc).unwrap();
        engine
            .install_cached_entry(KEY, row.serial, row.unit_id, row.slot)
            .unwrap()
    });
    (engine, units)
}

fn child(engine: &mut EngineInstance, unit: ResidentInstallation, pc: u32) -> u64 {
    engine.expect_execution_lookup(Some(unit));
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(unit.unit_id))
        .unwrap();
    let mut state = State32 {
        eip: pc,
        eflags: 2,
        ..State32::default()
    };
    state.registers[4] = 0x8800;
    let arena = engine.arena.as_mut().get_mut();
    encode_state_v3(&state, &mut arena[..STATE_SIZE]).unwrap();
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
    epoch
}

fn fold(engine: &mut EngineInstance, retired: u32) {
    header::write_u32(engine.arena.as_mut().get_mut(), EXIT_OFFSET + 20, retired);
}

#[test]
fn compiled_need_code_seeds_once_then_skips_full_extended_capture() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    child(&mut engine, units[0], 0x1020);
    assert!(!engine.has_compiled_light_continuation());
    child(&mut engine, units[1], 0x1000);
    assert!(engine.has_compiled_light_continuation());
    child(&mut engine, units[0], 0x1020);
    fold(&mut engine, 3);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!(
        (receipt.total_retired, receipt.child_retired, receipt.kind),
        (3, 1, 2)
    );
    let usage = engine.compiled_execution_usage(KEY).unwrap();
    assert_eq!(
        (
            usage.seed_full_need_code_seals,
            usage.light_need_code_seals,
            usage.full_materializations,
            usage.eligible_children
        ),
        (1, 2, 1, 3)
    );
}

#[test]
fn checked_and_c1_owned_need_code_never_issue_light_authority() {
    for owned in [false, true] {
        let (mut engine, units) = fixture();
        let frame = if owned {
            engine.begin_owned_execution_receipt(KEY, 8, units[0].unit_id, true)
        } else {
            engine.begin_execution_receipt(KEY, 8, units[0].unit_id, true)
        }
        .unwrap();
        child(&mut engine, units[0], 0x1020);
        child(&mut engine, units[1], 0x1000);
        assert!(!engine.has_compiled_light_continuation());
        fold(&mut engine, 2);
        engine.finish_execution_receipt(KEY, frame.token).unwrap();
        assert_eq!(
            engine.compiled_execution_usage(KEY).unwrap(),
            Default::default()
        );
    }
}

#[test]
fn initial_malformed_extended_record_cannot_seed_a_light_continuation() {
    let (mut engine, units) = fixture();
    engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    engine.expect_execution_lookup(Some(units[0]));
    let epoch = engine
        .begin_exception_detection(PendingOwner::Resident(units[0].unit_id))
        .unwrap();
    let arena = engine.arena.as_mut().get_mut();
    arena[X87_OFFSET] ^= 1;
    encode_exit_v7(
        &ExecutionExit {
            reason: ExitReason::NeedCode,
            retired: 0,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    assert_eq!(
        engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert!(!engine.has_compiled_light_continuation());
}

#[test]
fn light_seal_rejects_public_mutation_and_wrong_finish_is_neutral() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    child(&mut engine, units[0], 0x1020);
    child(&mut engine, units[1], 0x1000);
    fold(&mut engine, 2);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.arena_mut().map(|_| ()),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(
        engine.finish_execution_receipt(KEY + 1, frame.token),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.compiled_execution_usage(KEY),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert!(engine.has_compiled_light_continuation());
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
    let usage = engine.compiled_execution_usage(KEY).unwrap();
    assert_eq!(
        (
            usage.full_materializations,
            usage.rejected_public_transitions
        ),
        (1, 1)
    );
}

#[test]
fn valid_token_terminal_failure_materializes_revokes_and_allows_full_seal_retry() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    child(&mut engine, units[0], 0x1020);
    let epoch = child(&mut engine, units[1], 0x1000);
    fold(&mut engine, 2);
    let saved = engine.arena()[EXIT_OFFSET];
    engine.arena.as_mut().get_mut()[EXIT_OFFSET] ^= 1;
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert!(!engine.has_compiled_light_continuation());
    assert_eq!(engine.cpu_owned_execution(epoch), 0);
    assert_eq!(engine.execution_frame.unwrap().token, frame.token);
    engine.arena_mut().unwrap()[EXIT_OFFSET] = saved;
    assert_eq!(
        engine
            .finish_execution_receipt(KEY, frame.token)
            .unwrap()
            .total_retired,
        2
    );
    assert_eq!(
        engine
            .compiled_execution_usage(KEY)
            .unwrap()
            .full_materializations,
        1
    );
}

#[test]
fn unknown_writer_materializes_before_entry_then_reseeds_full() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    child(&mut engine, units[0], 0x1020);
    child(&mut engine, units[1], 0x2000);
    assert!(engine.has_compiled_light_continuation());
    child(&mut engine, units[2], 0x1000);
    assert!(!engine.has_compiled_light_continuation());
    child(&mut engine, units[0], 0x1020);
    assert!(engine.has_compiled_light_continuation());
    fold(&mut engine, 4);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
    let usage = engine.compiled_execution_usage(KEY).unwrap();
    assert_eq!(
        (
            usage.seed_full_need_code_seals,
            usage.strict_full_need_code_seals,
            usage.light_need_code_seals,
            usage.full_materializations,
            usage.strict_children
        ),
        (1, 1, 2, 2, 1)
    );
}

#[test]
fn wrong_child_installation_cannot_replace_a_light_witness() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    child(&mut engine, units[0], 0x1020);
    child(&mut engine, units[1], 0x1000);
    let before_epoch = engine.detection_epoch;
    engine.expect_execution_lookup(Some(units[0]));
    assert_eq!(
        engine.begin_exception_detection(PendingOwner::Resident(units[1].unit_id)),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.detection_epoch, before_epoch);
    assert!(engine.has_compiled_light_continuation());
    fold(&mut engine, 2);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
}

#[test]
fn forbidden_raw_extended_corruption_cannot_be_upgraded_or_authorized() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
        .unwrap();
    child(&mut engine, units[0], 0x1020);
    child(&mut engine, units[1], 0x1000);
    fold(&mut engine, 2);
    engine.arena.as_mut().get_mut()[X87_OFFSET] ^= 1;
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(engine.execution_frame.unwrap().token, frame.token);
}

// terminal-fold control; this does not exercise asynchronous cancel delivery.
#[test]
fn compiled_light_cancelled_terminal_requires_live_cancel_word() {
    use crate::abi::{arena::CANCEL_OFFSET, x86::encode_exit_v3};
    for cancelled in [false, true] {
        let (mut engine, units) = fixture();
        let frame = engine
            .begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true)
            .unwrap();
        child(&mut engine, units[0], 0x1020);
        child(&mut engine, units[1], 0x1000);
        assert!(engine.has_compiled_light_continuation());
        fold(&mut engine, 2);
        // a synthetic terminal fold must agree with the live cancel word.
        let arena = engine.arena.as_mut().get_mut();
        header::write_u32(arena, CANCEL_OFFSET, u32::from(cancelled));
        encode_exit_v3(
            &ExecutionExit {
                reason: ExitReason::Cancelled,
                retired: 2,
            },
            &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        if cancelled {
            let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
            assert_eq!(
                (
                    receipt.reason,
                    receipt.kind,
                    receipt.total_retired,
                    receipt.child_retired
                ),
                (2, 2, 2, 1)
            );
        } else {
            assert_eq!(
                engine.finish_execution_receipt(KEY, frame.token),
                Err(HostError::Call(CallError::StateChanged))
            );
            assert_eq!(engine.execution_frame.unwrap().token, frame.token);
            assert!(!engine.has_compiled_light_continuation());
            // repair only the rejected exit; the full seal remains pinned.
            encode_exit_v7(
                &ExecutionExit {
                    reason: ExitReason::NeedCode,
                    retired: 2,
                },
                &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
            )
            .unwrap();
            assert_eq!(
                engine
                    .finish_execution_receipt(KEY, frame.token)
                    .unwrap()
                    .total_retired,
                2
            );
        }
        let usage = engine.compiled_execution_usage(KEY).unwrap();
        assert_eq!(
            (
                usage.seed_full_need_code_seals,
                usage.light_need_code_seals,
                usage.full_materializations
            ),
            (1, 1, 1)
        );
    }
}
