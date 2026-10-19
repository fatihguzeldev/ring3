use crate::{
    abi::{
        arena::{EXIT_OFFSET, WORK_CONSUMED_OFFSET},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v7, encode_state_v3},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::{SegmentRegister, State32}},
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, ExecutionConfig32, HostError, ResidentInstallation,
        calls::call::PendingOwner,
    },
};

const KEY: u64 = 0x4e53_5543;

fn fixture() -> (EngineInstance, [ResidentInstallation; 3]) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for (pc, bytes) in [
        (0x1000, &[0xd9, 0x03, 0xd8, 0x03, 0xd9, 0x1b, 0xeb, 0xf8][..]),
        (0x1020, &[0x8b, 0x03, 0x83, 0xc0, 0x01, 0x89, 0x03, 0xeb, 0xf7][..]),
        (0x1040, &[0x0f, 0x57, 0xc0, 0xeb, 0xfb][..]),
    ] {
        engine.memory.as_mut().unwrap().write(GuestAddress(pc), bytes).unwrap();
    }
    engine.configure_execution(ExecutionConfig32 {
        units: 8,
        wasm_bytes: 262144,
        promotion_hits: 256,
    }).unwrap();
    let units = [0x1000, 0x1020, 0x1040].map(|pc| {
        let request = engine.prepare_cache_entry(KEY, pc).unwrap();
        engine.install_cached_entry(KEY, request.serial, request.unit_id, request.slot).unwrap()
    });
    (engine, units)
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    arena: Vec<u8>,
    output: Vec<u8>,
    output_pointer: usize,
    epoch: u64,
    owned_token: Option<u64>,
    frame: Option<(u64, Option<(u64, ResidentInstallation)>)>,
    authorization: Result<Option<ResidentInstallation>, HostError>,
    installations: Vec<Option<ResidentInstallation>>,
    source_version: Option<u64>,
    usage: [u64; 8],
}

fn snapshot(engine: &EngineInstance, unit: ResidentInstallation) -> Snapshot {
    let stats = &engine.compiled_execution_stats;
    Snapshot {
        arena: engine.arena().to_vec(),
        output: engine.native_success_module.clone(),
        output_pointer: engine.native_success_module.as_ptr() as usize,
        epoch: engine.detection_epoch,
        owned_token: engine.owned_receipt_token(),
        frame: engine.execution_frame.map(|frame| (frame.token, frame.last)),
        authorization: engine.authorize_execution_child(PendingOwner::Resident(unit.unit_id)),
        installations: engine.resident_installations.clone(),
        source_version: engine.memory.as_ref().map(|memory| memory.code_validation_version()),
        usage: [
            stats.seed_full_need_code_seals.get(), stats.strict_full_need_code_seals.get(),
            stats.light_need_code_seals.get(), stats.full_materializations.get(),
            stats.eligible_children.get(), stats.strict_children.get(),
            stats.continued_x87_step_attempts.get(), stats.rejected_public_transitions.get(),
        ],
    }
}

fn stop(engine: &mut EngineInstance, reason: ExitReason, pc: u32) {
    let mut state = State32 { eip: pc, eflags: 2, ..State32::default() };
    state.registers[3] = 0x8000;
    state.registers[4] = 0x8800;
    let arena = engine.arena.as_mut().get_mut();
    encode_state_v3(&state, &mut arena[..STATE_SIZE]).unwrap();
    encode_exit_v7(&ExecutionExit { reason, retired: 1 },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]).unwrap();
    header::write_u32(arena, WORK_CONSUMED_OFFSET, 1);
}

fn sealed(engine: &mut EngineInstance, unit: ResidentInstallation, next: u32) {
    engine.expect_execution_lookup(Some(unit));
    let epoch = engine.begin_exception_detection(PendingOwner::Resident(unit.unit_id)).unwrap();
    stop(engine, ExitReason::NeedCode, next);
    engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
}

#[test]
fn native_success_requires_an_actual_compiled_receipt_without_mutating_rejections() {
    for mode in 0..3 {
        let (mut engine, units) = fixture();
        if mode == 1 {
            engine.begin_execution_receipt(KEY, 8, units[0].unit_id, true).unwrap();
        } else if mode == 2 {
            engine.begin_owned_execution_receipt(KEY, 8, units[0].unit_id, true).unwrap();
        }
        let before = snapshot(&engine, units[0]);
        assert_eq!(engine.guard_native_success_receipt(), Err(HostError::InvalidRequest));
        assert_eq!(snapshot(&engine, units[0]), before);
    }
}

#[test]
fn compiled_guard_is_pure_and_mismatched_private_token_does_not_consume_entry() {
    let (mut engine, units) = fixture();
    let frame = engine.begin_compiled_execution_receipt(KEY, 1, units[0].unit_id, false).unwrap();
    let before = snapshot(&engine, units[0]);
    assert_eq!(engine.guard_native_success_receipt(), Ok(()));
    assert_eq!(snapshot(&engine, units[0]), before);
    engine.enable_compiled_execution(frame.token + 1);
    let mismatched = snapshot(&engine, units[0]);
    assert_eq!(engine.guard_native_success_receipt(), Err(HostError::Call(CallError::StateChanged)));
    assert_eq!(snapshot(&engine, units[0]), mismatched);
    engine.enable_compiled_execution(frame.token);
    assert_eq!(engine.guard_native_success_receipt(), Ok(()));
    assert_eq!(engine.begin_exception_detection(PendingOwner::Resident(units[0].unit_id)),
        Ok(frame.start_epoch + 1));
    stop(&mut engine, ExitReason::Budget, 0x1002);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!((receipt.kind, receipt.owner, receipt.total_retired), (1, units[0].unit_id, 1));
    assert_eq!(engine.guard_native_success_receipt(), Err(HostError::InvalidRequest));
}

#[test]
fn compiled_guard_rejects_a_missing_owned_lease_without_mutation() {
    let (mut engine, units) = fixture();
    engine.begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true).unwrap();
    engine.owned_execution = None;
    let before = snapshot(&engine, units[0]);
    assert_eq!(engine.guard_native_success_receipt(), Err(HostError::Call(CallError::StateChanged)));
    assert_eq!(snapshot(&engine, units[0]), before);
}

#[test]
fn sibling_output_is_bounded_and_does_not_replace_normal_resident_material() {
    let (mut engine, units) = fixture();
    for unit in &units[..2] {
        let original = engine.resident_bytes(unit.unit_id).unwrap().to_vec();
        let arena = engine.arena().to_vec();
        let epoch = engine.detection_epoch;
        let installations = engine.resident_installations.clone();
        let output = engine.prepare_native_success_module(KEY, unit.unit_id).unwrap().to_vec();
        assert!(output.starts_with(b"\0asm\x01\0\0\0"));
        assert!(output.len() <= 512 * 1024);
        assert!(output.windows(b"guard_detect_resident_native_success".len())
            .any(|window| window == b"guard_detect_resident_native_success"));
        assert_ne!(output, original);
        assert_eq!(engine.native_success_module, output);
        assert_eq!(engine.resident_bytes(unit.unit_id).unwrap(), original);
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.detection_epoch, epoch);
        assert_eq!(engine.resident_installations, installations);
    }
}

#[test]
fn failed_requests_preserve_previous_output_pointer_bytes_and_execution_state() {
    let (mut engine, units) = fixture();
    engine.prepare_native_success_module(KEY, units[0].unit_id).unwrap();
    for (key, id, error) in [
        (KEY ^ 1, units[0].unit_id, Some(HostError::InvalidArtifact)),
        (KEY, u64::MAX, Some(HostError::Resident(RegistryError::InvalidUnit))),
        (KEY, units[2].unit_id, None),
    ] {
        let before = snapshot(&engine, units[0]);
        let result = engine.prepare_native_success_module(key, id).map(|_| ());
        if let Some(error) = error { assert_eq!(result, Err(error)); } else { assert!(result.is_err()); }
        assert_eq!(snapshot(&engine, units[0]), before);
    }
}

#[test]
fn module_requests_reject_every_open_receipt_before_touching_output_or_witnesses() {
    for mode in 0..3 {
        for phase in 0..4 {
            let (mut engine, units) = fixture();
            engine.prepare_native_success_module(KEY, units[0].unit_id).unwrap();
            match mode {
                0 => engine.begin_execution_receipt(KEY, 8, units[0].unit_id, true),
                1 => engine.begin_owned_execution_receipt(KEY, 8, units[0].unit_id, true),
                _ => engine.begin_compiled_execution_receipt(KEY, 8, units[0].unit_id, true),
            }.unwrap();
            if phase == 1 {
                engine.expect_execution_lookup(Some(units[0]));
                engine.begin_exception_detection(PendingOwner::Resident(units[0].unit_id)).unwrap();
            } else if phase >= 2 {
                sealed(&mut engine, units[0], 0x1020);
                if phase == 3 { sealed(&mut engine, units[1], 0x1000); }
                if mode == 2 {
                    assert_eq!(engine.has_compiled_light_continuation(), phase == 3);
                }
            }
            let before = snapshot(&engine, units[0]);
            assert_eq!(engine.prepare_native_success_module(KEY, units[1].unit_id).map(|_| ()),
                Err(HostError::Call(CallError::Busy)), "mode {mode}, phase {phase}");
            assert_eq!(snapshot(&engine, units[0]), before, "mode {mode}, phase {phase}");
        }
    }
}

#[test]
fn module_request_rejects_legacy_active_detection_without_an_outer_receipt() {
    let (mut engine, units) = fixture();
    engine.prepare_native_success_module(KEY, units[0].unit_id).unwrap();
    let epoch = engine.begin_exception_detection(PendingOwner::Resident(units[0].unit_id)).unwrap();
    assert_eq!(epoch, 1);
    assert!(engine.execution_frame.is_none());
    let before = snapshot(&engine, units[0]);
    assert_eq!(engine.prepare_native_success_module(KEY, units[1].unit_id).map(|_| ()),
        Err(HostError::Call(CallError::Busy)));
    assert_eq!(snapshot(&engine, units[0]), before);
}

#[test]
fn stale_source_is_not_authorized_by_retained_sibling_bytes() {
    let (mut engine, units) = fixture();
    engine.prepare_native_success_module(KEY, units[0].unit_id).unwrap();
    engine.memory.as_mut().unwrap().write(GuestAddress(0x1000), &[0x90]).unwrap();
    let before = snapshot(&engine, units[0]);
    assert_eq!(engine.prepare_native_success_module(KEY, units[0].unit_id).map(|_| ()),
        Err(HostError::Resident(RegistryError::CodeInvalidated)));
    assert_eq!(snapshot(&engine, units[0]), before);
}

#[test]
fn changed_decode_context_rejects_emission_without_replacing_previous_output() {
    let (mut engine, units) = fixture();
    engine.prepare_native_success_module(KEY, units[0].unit_id).unwrap();
    engine.set_segment_base(SegmentRegister::Cs, 0x4000).unwrap();
    let before = snapshot(&engine, units[0]);
    assert_eq!(engine.prepare_native_success_module(KEY, units[0].unit_id).map(|_| ()),
        Err(HostError::InvalidArtifact));
    assert_eq!(snapshot(&engine, units[0]), before);
}

#[test]
fn close_releases_auxiliary_output_and_closed_request_is_neutral() {
    let (mut engine, units) = fixture();
    engine.prepare_native_success_module(KEY, units[0].unit_id).unwrap();
    assert!(!engine.native_success_module.is_empty());
    engine.close();
    assert!(engine.native_success_module.is_empty());
    let before = snapshot(&engine, units[0]);
    assert_eq!(engine.prepare_native_success_module(KEY, units[0].unit_id).map(|_| ()), Err(HostError::Closed));
    assert_eq!(snapshot(&engine, units[0]), before);
}
