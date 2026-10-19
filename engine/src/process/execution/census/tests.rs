use crate::{
    abi::{
        arena::{CANCEL_OFFSET, EXIT_OFFSET, WORK_CONSUMED_OFFSET},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_exit_v7, encode_state_v3},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, ExecutionConfig32, HostError, ResidentInstallation,
        calls::call::PendingOwner,
    },
};

const KEY: u64 = 0x4345_4e53;

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
            promotion_hits: 256,
        })
        .unwrap();
    let units = [0x1000, 0x1020].map(|pc| {
        let request = engine.prepare_cache_entry(KEY, pc).unwrap();
        engine
            .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
            .unwrap()
    });
    (engine, units)
}

fn state(engine: &mut EngineInstance, pc: u32) {
    let mut value = State32 {
        eip: pc,
        eflags: 2,
        ..State32::default()
    };
    value.registers[4] = 0x8800;
    encode_state_v3(&value, &mut engine.arena.as_mut().get_mut()[..STATE_SIZE]).unwrap();
}

fn enter(engine: &mut EngineInstance, unit: ResidentInstallation, pc: u32) -> u64 {
    state(engine, pc);
    header::write_u32(engine.arena.as_mut().get_mut(), WORK_CONSUMED_OFFSET, 0);
    engine.expect_execution_lookup(Some(unit));
    engine
        .begin_exception_detection(PendingOwner::Resident(unit.unit_id))
        .unwrap()
}

fn output(engine: &mut EngineInstance, pc: u32, reason: ExitReason, retired: u32, work: u32) {
    state(engine, pc);
    let arena = engine.arena.as_mut().get_mut();
    encode_exit_v7(
        &ExecutionExit { reason, retired },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    header::write_u32(arena, WORK_CONSUMED_OFFSET, work);
}

fn seal(engine: &mut EngineInstance, epoch: u64, reason: u32) {
    let mut fields = [0; 11];
    fields[0] = reason;
    engine.finish_generated_detection(epoch, fields).unwrap();
}

fn folded(engine: &mut EngineInstance, total: u32) {
    header::write_u32(engine.arena.as_mut().get_mut(), EXIT_OFFSET + 20, total);
}

fn one(
    engine: &mut EngineInstance,
    unit: ResidentInstallation,
    entry: u32,
    exit: u32,
    retired: u32,
    work: u32,
) {
    let frame = engine
        .begin_execution_receipt(KEY, retired.max(work) + 1, unit.unit_id, true)
        .unwrap();
    let epoch = enter(engine, unit, entry);
    output(engine, exit, ExitReason::NeedCode, retired, work);
    seal(engine, epoch, 3);
    folded(engine, retired);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn stopped(engine: &mut EngineInstance) -> [u8; 4096] {
    engine.stop_execution_census(KEY).unwrap();
    let packet = engine.execution_census_page(KEY, 0).unwrap();
    assert_eq!(&packet[..4], b"R3CE");
    assert_eq!(u32_at(&packet, 4), 0x0001_0001);
    assert_eq!(u32_at(&packet, 8), 4096);
    assert_eq!(u32_at(&packet, 12), 0);
    assert_eq!(u32_at(&packet, 24), 2);
    assert_eq!(u32_at(&packet, 44), 80);
    assert_eq!(u32_at(&packet, 124), 0);
    packet
}

#[test]
fn default_off_does_not_retain_earlier_children() {
    let (mut engine, units) = fixture();
    assert_eq!(
        engine.stop_execution_census(KEY),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.execution_census_page(KEY, 0),
        Err(HostError::InvalidRequest)
    );
    one(&mut engine, units[0], 0x1000, 0x1020, 2, 2);
    let before = engine.arena().to_vec();
    engine.begin_execution_census(KEY, 8).unwrap();
    assert_eq!(engine.arena(), before);
    let packet = stopped(&mut engine);
    assert_eq!(engine.arena(), before);
    assert_eq!(u64_at(&packet, 48), 0);
    assert_eq!(u32_at(&packet, 32), 0);
    assert!(packet[128..].iter().all(|byte| *byte == 0));
}

#[test]
fn retirement_varies_within_one_edge_instead_of_splitting_the_key() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    one(&mut engine, units[0], 0x1000, 0x1020, 2, 1);
    one(&mut engine, units[0], 0x1000, 0x1020, 5, 8);
    let packet = stopped(&mut engine);
    assert_eq!(u32_at(&packet, 32), 1);
    assert_eq!(u64_at(&packet, 48), 2);
    assert_eq!(u64_at(&packet, 56), 2);
    assert_eq!(u64_at(&packet, 64), 7);
    assert_eq!(u64_at(&packet, 72), 10);
    let row = &packet[128..208];
    assert_eq!(u64_at(row, 0), units[0].unit_id);
    assert_eq!((u32_at(row, 8), u32_at(row, 12)), (0x1000, 0x1020));
    assert_eq!((u32_at(row, 16), u32_at(row, 20)), (3, 1));
    assert_eq!(
        (u64_at(row, 24), u64_at(row, 32), u64_at(row, 40)),
        (2, 7, 10)
    );
    assert_eq!((u32_at(row, 48), u32_at(row, 52)), (2, 5));
    assert_eq!((u32_at(row, 56), u32_at(row, 60)), (1, 2));
    assert!(u32_at(row, 64) > 0);
    assert_eq!(
        (u32_at(row, 68), u32_at(row, 72), u32_at(row, 76)),
        (0, 1, 0)
    );
    assert_eq!(u32_at(&packet, 120), 0);
}

#[test]
fn need_code_chain_and_terminal_event_finalize_each_epoch_once() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 20, units[0].unit_id, true)
        .unwrap();
    for (unit, entry, exit, retired, work) in [
        (units[0], 0x1000, 0x1020, 2, 3),
        (units[1], 0x1020, 0x1000, 3, 4),
    ] {
        let epoch = enter(&mut engine, unit, entry);
        output(&mut engine, exit, ExitReason::NeedCode, retired, work);
        seal(&mut engine, epoch, 3);
    }
    let epoch = enter(&mut engine, units[0], 0x1000);
    output(&mut engine, 0x1000, ExitReason::InvalidOpcode, 0, 0);
    seal(&mut engine, epoch, 20);
    folded(&mut engine, 5);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!(
        (receipt.kind, receipt.total_retired, receipt.child_retired),
        (3, 5, 0)
    );
    let packet = stopped(&mut engine);
    assert_eq!((u64_at(&packet, 48), u64_at(&packet, 56)), (3, 3));
    assert_eq!((u64_at(&packet, 64), u64_at(&packet, 72)), (5, 7));
    assert_eq!(u32_at(&packet, 32), 3);
    let event = &packet[128 + 160..128 + 240];
    assert_eq!((u32_at(event, 16), u32_at(event, 20)), (20, 2));
    assert_eq!((u64_at(event, 24), u64_at(event, 32)), (1, 0));
    assert_eq!(u64_at(&packet, 104), 0);
}

#[test]
fn active_budget_uses_child_delta_and_work_after_prior_seals() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 9, units[0].unit_id, true)
        .unwrap();
    let first = enter(&mut engine, units[0], 0x1000);
    output(&mut engine, 0x1020, ExitReason::NeedCode, 2, 3);
    seal(&mut engine, first, 3);
    enter(&mut engine, units[1], 0x1020);
    output(&mut engine, 0x1021, ExitReason::Budget, 4, 6);
    folded(&mut engine, 6);
    let receipt = engine.finish_execution_receipt(KEY, frame.token).unwrap();
    assert_eq!((receipt.kind, receipt.child_retired), (1, 4));
    let packet = stopped(&mut engine);
    let active = &packet[208..288];
    assert_eq!((u32_at(active, 16), u32_at(active, 20)), (1, 3));
    assert_eq!((u64_at(active, 32), u64_at(active, 40)), (4, 6));
    assert_eq!((u64_at(&packet, 64), u64_at(&packet, 72)), (6, 9));
}

#[test]
fn outer_cancel_does_not_relabel_or_duplicate_a_sealed_need_code() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 10, units[0].unit_id, true)
        .unwrap();
    let epoch = enter(&mut engine, units[0], 0x1000);
    output(&mut engine, 0x1020, ExitReason::NeedCode, 2, 2);
    seal(&mut engine, epoch, 3);
    let arena = engine.arena.as_mut().get_mut();
    encode_exit_v3(
        &ExecutionExit {
            reason: ExitReason::Cancelled,
            retired: 2,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    header::write_u32(arena, CANCEL_OFFSET, 1);
    assert_eq!(
        engine
            .finish_execution_receipt(KEY, frame.token)
            .unwrap()
            .kind,
        2
    );
    let packet = stopped(&mut engine);
    assert_eq!((u32_at(&packet, 32), u64_at(&packet, 56)), (1, 1));
    assert_eq!((u32_at(&packet, 144), u32_at(&packet, 148)), (3, 1));
}

#[test]
fn zero_child_stops_have_no_owner_edge() {
    for (reason, budget, cancel) in [(ExitReason::Budget, 0, 0), (ExitReason::Cancelled, 10, 1)] {
        let (mut engine, units) = fixture();
        engine.begin_execution_census(KEY, 8).unwrap();
        let frame = engine
            .begin_execution_receipt(KEY, budget, units[0].unit_id, true)
            .unwrap();
        output(&mut engine, 0x1000, reason, 0, 0);
        header::write_u32(engine.arena.as_mut().get_mut(), CANCEL_OFFSET, cancel);
        assert_eq!(
            engine
                .finish_execution_receipt(KEY, frame.token)
                .unwrap()
                .kind,
            0
        );
        let packet = stopped(&mut engine);
        assert_eq!(
            (
                u64_at(&packet, 48),
                u64_at(&packet, 56),
                u64_at(&packet, 112)
            ),
            (0, 0, 1)
        );
        assert_eq!(u32_at(&packet, 32), 0);
    }
}

#[test]
fn key_busy_and_wrong_terminal_token_rejections_are_neutral() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 10, units[0].unit_id, true)
        .unwrap();
    enter(&mut engine, units[0], 0x1000);
    output(&mut engine, 0x1001, ExitReason::Budget, 1, 1);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.begin_execution_census(KEY + 1, 8),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.stop_execution_census(KEY + 1),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.execution_census_page(KEY + 1, 0),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.begin_execution_census(KEY, 0),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.stop_execution_census(KEY),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.execution_census_page(KEY, 0),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), before);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
    let packet = stopped(&mut engine);
    assert_eq!((u64_at(&packet, 48), u64_at(&packet, 56)), (1, 1));
    let frozen = engine.arena().to_vec();
    assert_eq!(
        engine.execution_census_page(KEY + 1, 0),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.execution_census_page(KEY, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), frozen);
    assert_eq!(engine.execution_census_page(KEY, 0).unwrap(), packet);
}

#[test]
fn rejected_seal_and_failed_finish_wait_for_valid_repair() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 10, units[0].unit_id, true)
        .unwrap();
    let epoch = enter(&mut engine, units[0], 0x1000);
    output(&mut engine, 0x1020, ExitReason::NeedCode, 2, 2);
    let mut invalid = [0; 11];
    invalid[0] = 3;
    invalid[1] = 1;
    assert_eq!(
        engine.finish_generated_detection(epoch, invalid),
        Err(HostError::InvalidRequest)
    );
    seal(&mut engine, epoch, 3);
    folded(&mut engine, 1);
    assert!(engine.finish_execution_receipt(KEY, frame.token).is_err());
    folded(&mut engine, 2);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
    let packet = stopped(&mut engine);
    assert_eq!(
        (
            u64_at(&packet, 48),
            u64_at(&packet, 56),
            u64_at(&packet, 64)
        ),
        (1, 1, 2)
    );
}

#[test]
fn active_terminal_is_recorded_only_after_late_validation_and_repair() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 10, units[0].unit_id, true)
        .unwrap();
    enter(&mut engine, units[0], 0x1000);
    output(&mut engine, 0x1020, ExitReason::Budget, 4, 4);
    engine.resident_installations[units[0].slot as usize] = None;
    assert_eq!(
        engine.finish_execution_receipt(KEY, frame.token),
        Err(HostError::InvalidArtifact)
    );
    engine.resident_installations[units[0].slot as usize] = Some(units[0]);
    output(&mut engine, 0x1001, ExitReason::Budget, 3, 3);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
    let packet = stopped(&mut engine);
    assert_eq!(
        (
            u64_at(&packet, 48),
            u64_at(&packet, 56),
            u64_at(&packet, 104)
        ),
        (1, 1, 0)
    );
    assert_eq!(u32_at(&packet, 140), 0x1001);
    assert_eq!((u64_at(&packet, 160), u64_at(&packet, 168)), (3, 3));
}

#[test]
fn full_table_retains_existing_keys_and_accounts_dropped_work() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 1).unwrap();
    one(&mut engine, units[0], 0x1000, 0x1020, 2, 2);
    one(&mut engine, units[1], 0x1020, 0x1000, 3, 3);
    one(&mut engine, units[0], 0x1000, 0x1020, 4, 4);
    let packet = stopped(&mut engine);
    assert_eq!((u64_at(&packet, 48), u64_at(&packet, 56)), (3, 3));
    assert_eq!((u64_at(&packet, 64), u64_at(&packet, 72)), (9, 9));
    assert_eq!(
        (
            u64_at(&packet, 80),
            u64_at(&packet, 88),
            u64_at(&packet, 96)
        ),
        (1, 3, 3)
    );
    assert_eq!(
        (
            u64_at(&packet, 152),
            u64_at(&packet, 160),
            u64_at(&packet, 168)
        ),
        (2, 6, 6)
    );
    assert_eq!((u32_at(&packet, 176), u32_at(&packet, 180)), (2, 4));
}

#[test]
fn unfinished_reentry_is_visible_instead_of_fabricating_an_edge() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    let frame = engine
        .begin_execution_receipt(KEY, 10, units[0].unit_id, true)
        .unwrap();
    enter(&mut engine, units[0], 0x1000);
    let epoch = enter(&mut engine, units[1], 0x1020);
    output(&mut engine, 0x1000, ExitReason::NeedCode, 2, 2);
    seal(&mut engine, epoch, 3);
    folded(&mut engine, 2);
    engine.finish_execution_receipt(KEY, frame.token).unwrap();
    let packet = stopped(&mut engine);
    assert_eq!(
        (
            u64_at(&packet, 48),
            u64_at(&packet, 56),
            u64_at(&packet, 104)
        ),
        (2, 1, 1)
    );
    assert_eq!(u32_at(&packet, 120) & 4, 4);
}

#[test]
fn actual_interior_entry_is_not_replaced_by_the_owner_root() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 8).unwrap();
    one(&mut engine, units[0], 0x1000, 0x1020, 2, 2);
    one(&mut engine, units[0], 0x1001, 0x1020, 1, 1);
    let packet = stopped(&mut engine);
    assert_eq!(u32_at(&packet, 32), 2);
    assert_eq!(
        (u32_at(&packet, 136), u32_at(&packet, 216)),
        (0x1000, 0x1001)
    );
}

#[test]
fn stopped_pages_are_stable_and_begin_issues_a_fresh_generation() {
    let (mut engine, units) = fixture();
    engine.begin_execution_census(KEY, 64).unwrap();
    for index in 0..53 {
        one(&mut engine, units[0], 0x1000, 0x2000 + index * 4, 1, 1);
    }
    let first = stopped(&mut engine);
    let second = engine.execution_census_page(KEY, 49).unwrap();
    assert_eq!((u32_at(&first, 32), u32_at(&first, 40)), (53, 49));
    assert_eq!((u32_at(&second, 36), u32_at(&second, 40)), (49, 4));
    assert_eq!(&first[..36], &second[..36]);
    assert_eq!(&first[44..128], &second[44..128]);
    assert!(second[128 + 4 * 80..].iter().all(|byte| *byte == 0));
    assert_eq!(engine.execution_census_page(KEY, 0).unwrap(), first);
    engine.stop_execution_census(KEY).unwrap();
    assert_eq!(engine.execution_census_page(KEY, 0).unwrap(), first);
    for capacity in [0, 32769] {
        assert_eq!(
            engine.begin_execution_census(KEY, capacity),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.execution_census_page(KEY, 0).unwrap(), first);
    }
    engine.begin_execution_census(KEY, 32768).unwrap();
    assert_eq!(
        engine.execution_census_page(KEY, 0),
        Err(HostError::Call(CallError::Busy))
    );
    let fresh = stopped(&mut engine);
    assert_eq!(u64_at(&fresh, 16), u64_at(&first, 16) + 1);
    assert_eq!((u32_at(&fresh, 32), u64_at(&fresh, 48)), (0, 0));
}
