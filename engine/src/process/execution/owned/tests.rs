use crate::{
    abi::{
        arena::{EXIT_OFFSET, SEGMENTS_OFFSET, WORK_CONSUMED_OFFSET, X87_OFFSET},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, X87_SIZE, encode_exit_v7, encode_state_v3, encode_x87},
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::RegistryError,
        x86::{SegmentDescriptorRecord, SegmentRegister, State32, X87Operation, X87State},
    },
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, ExecutionConfig32, HostError, StoreCompletion,
        calls::call::PendingOwner, execution::installation::ResidentInstallation,
    },
};

const KEY: u64 = 0x4f574e44;

fn fixture() -> (EngineInstance, [ResidentInstallation; 2]) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x2000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for pc in [0x1000, 0x2000] {
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
            promotion_hits: 256,
        })
        .unwrap();
    let installed = [0x1000, 0x2000].map(|pc| {
        let row = engine.prepare_cache_entry(KEY, pc).unwrap();
        engine
            .install_cached_entry(KEY, row.serial, row.unit_id, row.slot)
            .unwrap()
    });
    (engine, installed)
}

fn active(engine: &mut EngineInstance, unit: ResidentInstallation) -> u64 {
    engine
        .begin_exception_detection(PendingOwner::Resident(unit.unit_id))
        .unwrap()
}

fn budget_stop(engine: &mut EngineInstance, retired: u32) {
    let mut state = State32 {
        eip: 0x1001,
        eflags: 2,
        ..State32::default()
    };
    state.registers[4] = 0x8800;
    let arena = engine.arena.as_mut().get_mut();
    encode_state_v3(&state, &mut arena[..STATE_SIZE]).unwrap();
    encode_exit_v7(
        &ExecutionExit {
            reason: ExitReason::Budget,
            retired,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    header::write_u32(arena, WORK_CONSUMED_OFFSET, retired);
}

#[test]
fn explicit_owned_entry_is_required_and_ends_with_the_receipt_or_close() {
    for owned in [false, true] {
        let (mut engine, units) = fixture();
        let frame = if owned {
            engine.begin_owned_execution_receipt(KEY, 1, units[0].unit_id, false)
        } else {
            engine.begin_execution_receipt(KEY, 1, units[0].unit_id, false)
        }
        .unwrap();
        assert_eq!(engine.cpu_owned_execution(0), 0);
        let epoch = active(&mut engine, units[0]);
        assert_eq!(engine.cpu_owned_execution(epoch), u32::from(owned));
        assert_eq!(engine.cpu_owned_execution(epoch + 1), 0);
        assert_eq!(engine.cpu_owned_execution(0), 0);
        budget_stop(&mut engine, 1);
        let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
        assert_eq!((receipt.epoch, receipt.total_retired), (epoch, 1));
        assert_eq!(engine.cpu_owned_execution(epoch), 0);
    }
    let (mut engine, units) = fixture();
    engine
        .begin_owned_execution_receipt(KEY, 1, units[0].unit_id, false)
        .unwrap();
    let epoch = active(&mut engine, units[0]);
    assert_eq!(engine.cpu_owned_execution(epoch), 1);
    engine.close();
    assert_eq!(engine.cpu_owned_execution(epoch), 0);
}

#[test]
fn rejected_owned_entry_does_not_issue_a_frame_or_enable_an_active_checked_child() {
    let (mut engine, units) = fixture();
    let before = engine.arena().to_vec();
    for (key, unit) in [(KEY ^ 1, units[0].unit_id), (KEY, u64::MAX)] {
        assert!(
            engine
                .begin_owned_execution_receipt(key, 1, unit, false)
                .is_err()
        );
        assert!(engine.execution_frame.is_none());
        assert_eq!(engine.execution_frame_issuer, 0);
        assert_eq!(engine.arena(), before);
    }
    engine
        .begin_execution_receipt(KEY, 1, units[0].unit_id, false)
        .unwrap();
    let epoch = active(&mut engine, units[0]);
    assert_eq!(
        engine.begin_owned_execution_receipt(KEY, 1, units[0].unit_id, false),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.cpu_owned_execution(epoch), 0);
}

#[test]
fn owned_material_revalidates_source_before_reusing_authority_and_rejects_wrong_identity() {
    let (mut engine, units) = fixture();
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, false)
        .unwrap();
    active(&mut engine, units[0]);
    assert_eq!(
        engine.guard_material_resident(KEY, units[0].unit_id),
        Ok(())
    );
    assert_eq!(
        engine.guard_material_resident(KEY, units[0].unit_id),
        Ok(())
    );
    assert_eq!(
        engine.guard_material_resident(KEY ^ 1, units[0].unit_id),
        Err(HostError::InvalidArtifact)
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x91])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.guard_material_resident(KEY, units[0].unit_id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn admitted_segment_load_invalidates_the_owned_operand_context() {
    let (mut engine, units) = fixture();
    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0x4000_ffff, 0x0040_f200),
        )
        .unwrap();
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, false)
        .unwrap();
    active(&mut engine, units[0]);
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    let before = engine.operand_segment_context().unwrap();
    assert_ne!(before.state.selectors[SegmentRegister::Fs.index()], 0x43);
    assert_eq!(engine.operand_segment_context(), Ok(before));
    engine
        .segment_load_resident(
            KEY,
            units[0].unit_id,
            SegmentRegister::Fs.index() as u32,
            0x43,
        )
        .unwrap();
    let after = engine.operand_segment_context().unwrap();
    assert_eq!(after.state.selectors[SegmentRegister::Fs.index()], 0x43);
    assert_eq!(after.state.bases[SegmentRegister::Fs.index()], 0x4000);
    assert_eq!(after, engine.segment_context().unwrap());
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
}

#[test]
fn owned_host_mapping_configuration_and_registry_mutations_are_neutral_while_running() {
    let (mut engine, units) = fixture();
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, false)
        .unwrap();
    active(&mut engine, units[0]);
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    engine.operand_segment_context().unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    let busy = Err(HostError::Call(CallError::Busy));
    assert_eq!(engine.map(0x6000, 1, 3), busy);
    assert_eq!(engine.protect(0x5000, 1, 1), busy);
    assert_eq!(engine.unmap(0x5000, 1), busy);
    assert_eq!(engine.set_segment_base(SegmentRegister::Fs, 0x9000), busy);
    assert_eq!(engine.schedule_windows_thread().map(|_| ()), busy);
    assert_eq!(
        engine.discard_installed_resident(KEY, units[1].unit_id, units[1].slot),
        busy
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        engine.resident_installation_by_id(units[1].unit_id),
        Some(units[1])
    );
}

#[test]
fn owned_context_is_lazy_so_pending_mf_precedes_a_malformed_operand_context() {
    let (mut engine, units) = fixture();
    let state = X87State {
        control: 0x037e,
        status: 1,
        ..X87State::default()
    };
    let arena = engine.arena.as_mut().get_mut();
    encode_x87(&state, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    arena[SEGMENTS_OFFSET + 60] = 1;
    assert_eq!(
        engine.operand_segment_context(),
        Err(HostError::Infrastructure)
    );
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, false)
        .unwrap();
    active(&mut engine, units[0]);
    assert_eq!(
        engine.x87_resident_op(
            KEY,
            units[0].unit_id,
            X87Operation::Wait.encode(),
            0x6000,
            2,
            0x1000,
            0,
            1,
        ),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.cr2(), None);
    assert_eq!(
        u16::from_le_bytes(
            engine.arena()[X87_OFFSET + 18..X87_OFFSET + 20]
                .try_into()
                .unwrap()
        ),
        0x8081
    );
}

#[test]
fn public_arena_mutation_invalidates_a_primed_owned_operand_context() {
    let (mut engine, units) = fixture();
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, false)
        .unwrap();
    active(&mut engine, units[0]);
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    engine.operand_segment_context().unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET + 60] = 1;
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    assert_eq!(
        engine.operand_segment_context(),
        Err(HostError::Infrastructure)
    );
}

#[test]
fn dispatched_child_handoff_requires_its_new_exact_epoch_and_material_owner() {
    let (mut engine, units) = fixture();
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, true)
        .unwrap();
    engine.expect_execution_lookup(Some(units[0]));
    let first = active(&mut engine, units[0]);
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    engine.operand_segment_context().unwrap();
    engine.expect_execution_lookup(Some(units[1]));
    let second = active(&mut engine, units[1]);
    assert_ne!(first, second);
    assert_eq!(engine.cpu_owned_execution(first), 0);
    assert_eq!(engine.cpu_owned_execution(second), 1);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x91])
        .unwrap();
    engine
        .guard_material_resident(KEY, units[1].unit_id)
        .unwrap();
    assert_eq!(
        engine.guard_material_resident(KEY, units[0].unit_id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
}

#[test]
fn valid_terminal_attempt_revokes_owned_authority_but_preserves_a_failed_receipt_for_retry() {
    let (mut engine, units) = fixture();
    let frame = engine
        .begin_owned_execution_receipt(KEY, 1, units[0].unit_id, false)
        .unwrap();
    let epoch = active(&mut engine, units[0]);
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    engine.operand_segment_context().unwrap();
    budget_stop(&mut engine, 1);
    let valid = engine.arena().to_vec();
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.cpu_owned_execution(epoch), 1);
    assert_eq!(engine.arena(), valid);
    engine.arena.as_mut().get_mut()[EXIT_OFFSET] = 0;
    let invalid = engine.arena().to_vec();
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(engine.cpu_owned_execution(epoch), 0);
    assert_eq!(engine.execution_frame.as_ref().unwrap().token, frame.token);
    assert_eq!(engine.arena(), invalid);
    engine.arena_mut().unwrap().copy_from_slice(&valid);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!((receipt.epoch, receipt.total_retired), (epoch, 1));
}

#[test]
fn direct_windows_context_publication_revokes_owned_authority_before_replacing_segments() {
    let (mut engine, units) = fixture();
    engine
        .begin_owned_execution_receipt(KEY, 10, units[0].unit_id, false)
        .unwrap();
    let epoch = active(&mut engine, units[0]);
    engine
        .guard_material_resident(KEY, units[0].unit_id)
        .unwrap();
    engine.operand_segment_context().unwrap();
    let mut context = engine.windows_context_state().unwrap();
    context.segments.state.bases[SegmentRegister::Fs.index()] = 0x9000;
    engine.publish_windows_context(&context).unwrap();
    assert_eq!(engine.cpu_owned_execution(epoch), 0);
    assert_eq!(engine.operand_segment_context(), Ok(context.segments));
}
