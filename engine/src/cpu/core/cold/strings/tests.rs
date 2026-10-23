use super::*;
use crate::memory::{AddressSpace, GuestAddress, PageRange, Permissions};

fn memory(destination: Permissions) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for (pc, permissions) in [(0x3000, Permissions::READ_WRITE), (0x4000, destination)] {
        memory
            .map_zeroed(PageRange::new(GuestAddress(pc), 1).unwrap(), permissions)
            .unwrap();
    }
    memory
        .write(GuestAddress(0x3000), &[0x11, 0x22, 0x33])
        .unwrap();
    memory
}

fn state() -> State32 {
    State32 {
        registers: [7, 3, 0, 0, 0x4800, 0, 0x3000, 0x4000],
        eip: 0x1000,
        eflags: 2,
    }
}

fn operation(kind: StringKind, size: AddressSize, repeat: RepeatCondition) -> Operation {
    Operation::RepeatString {
        kind,
        width: StringWidth::Byte,
        repeat,
        address_size: size,
        source_segment: None,
    }
}

fn run(
    memory: &mut AddressSpace,
    state: &mut State32,
    operation: Operation,
    budget: u32,
) -> Progress {
    let segments = crate::cpu::x86::SegmentContext::default();
    let mut port = Port {
        memory,
        segments: &segments,
        code_written: false,
    };
    execute(operation, 0x1000, 0x1002, state, &mut port, budget).unwrap()
}

#[test]
fn repeat_budget_preserves_partial_work_without_retiring_the_opcode() {
    let mut memory = memory(Permissions::READ_WRITE);
    let mut state = state();
    let version = memory.transaction_version();
    let op = operation(
        StringKind::Move,
        AddressSize::Bits32,
        RepeatCondition::Always,
    );
    let first = run(&mut memory, &mut state, op, 2);
    assert_eq!(
        (
            first.retired,
            first.work,
            first.instruction_completed,
            first.stop
        ),
        (0, 2, false, Some(StopReason::Budget))
    );
    assert!(first.failure.is_none());
    assert_eq!(
        (
            state.eip,
            state.registers[1],
            state.registers[6],
            state.registers[7],
            state.eflags
        ),
        (0x1000, 1, 0x3002, 0x4002, 2)
    );
    assert_eq!(memory.transaction_version(), version + 2);
    let last = run(&mut memory, &mut state, op, 1);
    assert_eq!(
        (
            last.retired,
            last.work,
            last.instruction_completed,
            last.stop
        ),
        (1, 1, true, None)
    );
    assert_eq!(
        (
            state.eip,
            state.registers[1],
            state.registers[6],
            state.registers[7]
        ),
        (0x1002, 0, 0x3003, 0x4003)
    );
    let mut bytes = [0; 3];
    memory.read(GuestAddress(0x4000), &mut bytes).unwrap();
    assert_eq!(bytes, [0x11, 0x22, 0x33]);
    assert_eq!(memory.transaction_version(), version + 3);
}

#[test]
fn a_late_fault_keeps_only_committed_elements_and_exact_work() {
    let mut memory = memory(Permissions::READ_WRITE);
    let mut state = state();
    state.registers[7] = 0x4fff;
    let version = memory.transaction_version();
    let progress = run(
        &mut memory,
        &mut state,
        operation(
            StringKind::Move,
            AddressSize::Bits32,
            RepeatCondition::Always,
        ),
        3,
    );
    assert_eq!(
        (
            progress.retired,
            progress.work,
            progress.instruction_completed,
            progress.stop
        ),
        (0, 1, false, None)
    );
    assert!(matches!(progress.failure, Some(Failure::Memory(_, 1))));
    assert_eq!(
        (
            state.eip,
            state.registers[1],
            state.registers[6],
            state.registers[7],
            state.eflags
        ),
        (0x1000, 2, 0x3001, 0x5000, 2)
    );
    let mut byte = [0];
    memory.read(GuestAddress(0x4fff), &mut byte).unwrap();
    assert_eq!(byte, [0x11]);
    assert_eq!(memory.transaction_version(), version + 1);
}

#[test]
fn zero_selected_count_completes_without_access_and_budget_zero_precedes_it() {
    let mut memory = AddressSpace::new(1).unwrap();
    let mut state = state();
    state.registers[1] = 0xabcd_0000;
    state.registers[6] = u32::MAX;
    state.registers[7] = u32::MAX;
    let original = state;
    let op = operation(
        StringKind::Move,
        AddressSize::Bits16,
        RepeatCondition::Always,
    );
    let none = run(&mut memory, &mut state, op, 0);
    assert_eq!(
        (
            none.retired,
            none.work,
            none.instruction_completed,
            none.stop
        ),
        (0, 0, false, Some(StopReason::Budget))
    );
    assert_eq!(state, original);
    let done = run(&mut memory, &mut state, op, 1);
    assert_eq!(
        (
            done.retired,
            done.work,
            done.instruction_completed,
            done.stop
        ),
        (1, 1, true, None)
    );
    assert_eq!(state.registers, original.registers);
    assert_eq!(state.eip, 0x1002);
}

#[test]
fn word_address_direction_wrap_preserves_upper_register_halves() {
    let mut memory = memory(Permissions::READ_WRITE);
    let mut state = state();
    state.registers[1] = 0xabcd_0002;
    state.registers[6] = 0x1234_3001;
    state.registers[7] = 0xbeef_4001;
    state.eflags = 0x402;
    let progress = run(
        &mut memory,
        &mut state,
        operation(
            StringKind::Move,
            AddressSize::Bits16,
            RepeatCondition::Always,
        ),
        2,
    );
    assert_eq!(
        (
            progress.retired,
            progress.work,
            progress.instruction_completed
        ),
        (1, 2, true)
    );
    assert_eq!(
        (
            state.registers[1],
            state.registers[6],
            state.registers[7],
            state.eflags
        ),
        (0xabcd_0000, 0x1234_2fff, 0xbeef_3fff, 0x402)
    );
    let mut bytes = [0; 2];
    memory.read(GuestAddress(0x4000), &mut bytes).unwrap();
    assert_eq!(bytes, [0x11, 0x22]);
}

#[test]
fn comparisons_stop_on_the_actual_last_flags_and_count() {
    for (kind, repeat, expected_flags) in [
        (StringKind::Compare, RepeatCondition::Equal, 0x97),
        (StringKind::Scan, RepeatCondition::NotEqual, 0x46),
    ] {
        let mut memory = memory(Permissions::READ_WRITE);
        memory.write(GuestAddress(0x3000), &[1, 7, 9]).unwrap();
        memory
            .write(
                GuestAddress(0x4000),
                if kind == StringKind::Compare {
                    &[1, 8, 9]
                } else {
                    &[1, 7, 9]
                },
            )
            .unwrap();
        let mut state = state();
        let progress = run(
            &mut memory,
            &mut state,
            operation(kind, AddressSize::Bits32, repeat),
            8,
        );
        assert_eq!(
            (
                progress.retired,
                progress.work,
                progress.instruction_completed
            ),
            (1, 2, true)
        );
        assert_eq!(
            (
                state.eip,
                state.registers[1],
                state.registers[7],
                state.eflags
            ),
            (0x1002, 1, 0x4002, expected_flags)
        );
        assert_eq!(
            state.registers[6],
            if kind == StringKind::Compare {
                0x3002
            } else {
                0x3000
            }
        );
    }
}

#[test]
fn executable_store_reports_partial_or_completed_state_before_any_next_element() {
    let mut memory = memory(Permissions::ALL);
    let mut state = state();
    state.registers[1] = 2;
    let op = operation(
        StringKind::Move,
        AddressSize::Bits32,
        RepeatCondition::Always,
    );
    let first = run(&mut memory, &mut state, op, 8);
    assert_eq!(
        (
            first.retired,
            first.work,
            first.instruction_completed,
            first.stop
        ),
        (0, 1, false, Some(StopReason::CodeInvalidated))
    );
    assert_eq!(
        (state.eip, state.registers[1], state.registers[7]),
        (0x1000, 1, 0x4001)
    );
    let last = run(&mut memory, &mut state, op, 8);
    assert_eq!(
        (
            last.retired,
            last.work,
            last.instruction_completed,
            last.stop
        ),
        (1, 1, true, Some(StopReason::CodeInvalidated))
    );
    assert_eq!(
        (state.eip, state.registers[1], state.registers[7]),
        (0x1002, 0, 0x4002)
    );
}

#[test]
fn an_explicit_gs_source_uses_its_actual_segment_base() {
    let mut memory = memory(Permissions::READ_WRITE);
    let mut state = state();
    state.registers[6] = 0;
    let mut segments = crate::cpu::x86::SegmentContext::default();
    segments.caches[SegmentRegister::Gs.index()] = segments.caches[SegmentRegister::Ds.index()];
    segments.state.bases[SegmentRegister::Gs.index()] = 0x3000;
    let mut port = Port {
        memory: &mut memory,
        segments: &segments,
        code_written: false,
    };
    let op = Operation::String {
        kind: StringKind::Move,
        width: StringWidth::Byte,
        address_size: AddressSize::Bits32,
        source_segment: Some(SegmentRegister::Gs),
    };
    let result = execute(op, 0x1000, 0x1002, &mut state, &mut port, 1).unwrap();
    assert_eq!(
        (result.retired, result.work, result.instruction_completed),
        (1, 1, true)
    );
    assert!(result.failure.is_none());
    let mut byte = [0];
    memory.read(GuestAddress(0x4000), &mut byte).unwrap();
    assert_eq!(byte, [0x11]);
}
