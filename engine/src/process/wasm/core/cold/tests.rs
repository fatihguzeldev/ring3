use super::*;
use crate::cpu::x86::State32;

fn fixture(code: &[u8], budget: u32) -> (EngineInstance, Context32, NumericBank, SegmentContext) {
    let mut instance = EngineInstance::new(4, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x3000, 1, 3).unwrap();
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), code)
        .unwrap();
    let mut context = Context32::new(State32 {
        registers: [0, 0, 0, 0x3000, 0x3800, 0, 0, 0x5000],
        eip: 0x1000,
        ..State32::default()
    });
    context.set_word(44, budget);
    context.set_word(112, 1);
    let bank = NumericBank::from_instance(&instance).unwrap();
    let segments = instance.segment_context().unwrap();
    (instance, context, bank, segments)
}

fn execute(
    instance: &mut EngineInstance,
    context: &mut Context32,
    bank: &mut NumericBank,
    segments: &SegmentContext,
    gates: &[(GuestAddress, u32)],
) -> StopReason {
    run(
        instance,
        context,
        bank,
        segments,
        gates,
        &[],
        &mut ColdState::default(),
    )
}

#[test]
fn scalar_cold_execution_preserves_cumulative_retirement_and_concrete_flags() {
    let (mut instance, mut context, mut bank, segments) =
        fixture(&[0xb8, 3, 0, 0, 0, 0x83, 0xc0, 2], 2);
    context.set_word(40, 20);
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::Budget
    );
    assert_eq!(context.state().registers[0], 5);
    assert_eq!(context.state().eflags, 6);
    assert_eq!(context.state().eip, 0x1008);
    assert_eq!((context.retired(), context.remaining()), (22, 0));
}

#[test]
fn native_numerical_handoffs_keep_the_bank_between_cold_slices() {
    let (mut instance, mut context, mut bank, segments) =
        fixture(&[0xd9, 0x03, 0xd8, 0x4b, 4, 0xd9, 0x5b, 8], 2);
    let memory = instance.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(0x3000), &3_f32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x3004), &2_f32.to_le_bytes())
        .unwrap();
    let arena = instance.arena().to_vec();
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::Budget
    );
    assert_eq!((context.state().eip, context.retired()), (0x1005, 2));
    assert_eq!(
        bank.architectural_state().0.registers[7],
        [0, 0, 0, 0, 0, 0, 0, 0xc0, 1, 0x40]
    );
    assert_eq!(instance.arena(), arena);
    context.set_word(44, 1);
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::Budget
    );
    assert_eq!((context.state().eip, context.retired()), (0x1008, 3));
    assert_eq!(bank.architectural_state().0.tag, 0xffff);
    let mut output = [0; 4];
    instance
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x3008), &mut output)
        .unwrap();
    assert_eq!(output, 6_f32.to_le_bytes());
}

#[test]
fn a_native_store_fault_preserves_its_unretired_numerical_state() {
    let (mut instance, mut context, mut bank, segments) = fixture(&[0xd9, 0x03, 0xd9, 0x1f], 1);
    instance
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &3_f32.to_le_bytes())
        .unwrap();
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::Budget
    );
    let before = bank.architectural_state();
    context.set_word(44, 1);
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::MemoryFault
    );
    assert_eq!(bank.architectural_state(), before);
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1002, 1, 1)
    );
    assert_eq!(
        u32::from_le_bytes(context.bytes()[48..52].try_into().unwrap()),
        0x5000
    );
    assert_eq!(
        u32::from_le_bytes(context.bytes()[52..56].try_into().unwrap()),
        0x10204
    );
}

#[test]
fn a_real_gate_stops_without_retiring_the_stub() {
    let (mut instance, mut context, mut bank, segments) = fixture(&[0x0f, 0x0b], 8);
    assert_eq!(
        execute(
            &mut instance,
            &mut context,
            &mut bank,
            &segments,
            &[(GuestAddress(0x1000), 77)]
        ),
        StopReason::Host
    );
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1000, 0, 8)
    );
    assert_eq!(
        u32::from_le_bytes(context.bytes()[116..120].try_into().unwrap()),
        77
    );
}

#[test]
fn a_completed_system_entry_is_retired_once_before_the_host_boundary() {
    let (mut instance, mut context, mut bank, segments) = fixture(&[0xcd, 0x80], 1);
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::SystemEntry
    );
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1002, 1, 0)
    );
    assert_eq!(
        u32::from_le_bytes(context.bytes()[48..52].try_into().unwrap()),
        0x1000
    );
    assert_eq!(
        u32::from_le_bytes(context.bytes()[116..120].try_into().unwrap()),
        0x80
    );
}

#[test]
fn a_zero_budget_cold_slice_performs_no_guest_or_numerical_work() {
    let (mut instance, mut context, mut bank, segments) = fixture(&[0xd9, 0x03], 0);
    let state = *context.bytes();
    let numerical = bank.architectural_state();
    let arena = instance.arena().to_vec();
    assert_eq!(
        execute(&mut instance, &mut context, &mut bank, &segments, &[]),
        StopReason::Budget
    );
    assert_eq!(*context.bytes(), state);
    assert_eq!(bank.architectural_state(), numerical);
    assert_eq!(instance.arena(), arena);
}
