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

#[cfg(target_arch = "wasm32")]
use super::super::super::cache::CoreCache32;
#[cfg(not(target_arch = "wasm32"))]
use crate::process::core_cache_tests::CoreCache32;
use crate::{
    cpu::core::{CompileOptions, compile, compile_with_gates},
    memory::{AddressSpace, PageRange, Permissions},
};

fn frontier_fixture(budget: u32) -> (EngineInstance, Context32, NumericBank, SegmentContext) {
    let mut fixture = fixture(&[0x40, 0xeb, 0x0d], budget);
    let memory = fixture.0.memory.as_mut().unwrap();
    memory.write(GuestAddress(0x1008), &[0xeb, 6]).unwrap();
    memory
        .write(GuestAddress(0x1010), &[0x43, 0x0f, 0x0b])
        .unwrap();
    memory
        .write(GuestAddress(0x1080), &[0x90, 0x0f, 0x0b])
        .unwrap();
    fixture
}

fn frontier_options(instructions: usize) -> CompileOptions {
    CompileOptions {
        instructions,
        blocks: 2,
        ..CompileOptions::default()
    }
}

fn frontier_pages(memory: &AddressSpace) -> [[u8; 4096]; 2] {
    let mut pages = [[0; 4096]; 2];
    for (address, page) in [0x1000, 0x3000].into_iter().zip(&mut pages) {
        memory.read(GuestAddress(address), page).unwrap();
    }
    pages
}

#[test]
fn cold_prefix_reaches_a_retained_side_entry_without_executing_it() {
    for linked in [false, true] {
        let (mut instance, mut context, mut bank, segments) = frontier_fixture(3);
        let mut cache = CoreCache32::default();
        let owner = cache.insert(
            compile(
                instance.memory.as_ref().unwrap(),
                GuestAddress(0x1008),
                frontier_options(2),
            )
            .unwrap(),
        );
        assert_eq!(
            owner.entrypoints(),
            &[GuestAddress(0x1008), GuestAddress(0x1010)]
        );
        assert!(!cache.frontier_contains(instance.memory.as_ref().unwrap(), 0x1000));
        if linked {
            cache.install_link(&owner, 1).unwrap();
        }
        let status = cache.snapshot();
        let victim = cache.oldest_linked_slot();
        let numerical = bank.architectural_state();
        let arena = instance.arena().to_vec();
        let pages = frontier_pages(instance.memory.as_ref().unwrap());
        let mut expected = *context.bytes();
        for (offset, value) in [(0, 1_u32), (32, 0x1010), (40, 2), (44, 1)] {
            expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            run_with_frontier(
                &mut instance,
                &mut context,
                &mut bank,
                &segments,
                &[],
                &|memory, pc| cache.frontier_contains(memory, pc),
                &mut ColdState::default(),
            ),
            StopReason::NeedCode
        );
        assert_eq!(*context.bytes(), expected);
        assert_eq!(bank.architectural_state(), numerical);
        assert_eq!(instance.arena(), arena);
        assert_eq!(frontier_pages(instance.memory.as_ref().unwrap()), pages);
        assert_eq!(cache.snapshot(), status);
        assert_eq!(cache.oldest_linked_slot(), victim);
    }
}

#[test]
fn cold_prefix_ignores_stale_remapped_and_reset_cache_authority() {
    for mode in 0..3 {
        let (mut instance, mut context, mut bank, segments) = frontier_fixture(3);
        let mut cache = CoreCache32::default();
        let old = cache.insert(
            compile(
                instance.memory.as_ref().unwrap(),
                GuestAddress(0x1008),
                frontier_options(2),
            )
            .unwrap(),
        );
        let memory = instance.memory.as_mut().unwrap();
        match mode {
            0 => memory.write(GuestAddress(0x1010), &[0x42]).unwrap(),
            1 => {
                let page = frontier_pages(memory)[0];
                let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
                memory.unmap(range).unwrap();
                memory.map_zeroed(range, Permissions::ALL).unwrap();
                memory.write(GuestAddress(0x1000), &page).unwrap();
            }
            _ => cache = CoreCache32::default(),
        }
        assert_eq!(old.is_current(memory), mode == 2);
        assert!(!cache.frontier_contains(memory, 0x1010));
        let status = cache.snapshot();
        let numerical = bank.architectural_state();
        let pages = frontier_pages(memory);
        let arena = instance.arena().to_vec();
        assert_eq!(
            run_with_frontier(
                &mut instance,
                &mut context,
                &mut bank,
                &segments,
                &[],
                &|memory, pc| cache.frontier_contains(memory, pc),
                &mut ColdState::default(),
            ),
            StopReason::Budget
        );
        let mut registers = [1, 0, 0, 0x3000, 0x3800, 0, 0, 0x5000];
        registers[if mode == 0 { 2 } else { 3 }] += 1;
        assert_eq!(
            context.state(),
            State32 {
                registers,
                eip: 0x1011,
                eflags: 2
            }
        );
        assert_eq!((context.retired(), context.remaining()), (3, 0));
        assert_eq!(bank.architectural_state(), numerical);
        assert_eq!(instance.arena(), arena);
        assert_eq!(frontier_pages(instance.memory.as_ref().unwrap()), pages);
        assert_eq!(cache.snapshot(), status);
    }
}

#[test]
fn an_evicted_but_held_owner_does_not_stop_the_cold_prefix() {
    let (mut instance, mut context, mut bank, segments) = frontier_fixture(3);
    let mut cache = CoreCache32::default();
    let memory = instance.memory.as_ref().unwrap();
    let old = cache.insert(compile(memory, GuestAddress(0x1008), frontier_options(2)).unwrap());
    let capacity = cache.snapshot()[7];
    for _ in 0..capacity {
        cache.insert(compile(memory, GuestAddress(0x1080), frontier_options(1)).unwrap());
    }
    assert!(old.is_current(memory));
    assert_eq!(
        cache.snapshot()[2],
        1,
        "actual count pressure evicted one owner"
    );
    assert!(!cache.frontier_contains(memory, 0x1010));
    let status = cache.snapshot();
    assert_eq!(
        run_with_frontier(
            &mut instance,
            &mut context,
            &mut bank,
            &segments,
            &[],
            &|memory, pc| cache.frontier_contains(memory, pc),
            &mut ColdState::default(),
        ),
        StopReason::Budget
    );
    assert_eq!(
        (context.state().eip, context.state().registers[3]),
        (0x1011, 0x3001)
    );
    assert_eq!((context.retired(), context.remaining()), (3, 0));
    assert_eq!(cache.snapshot(), status);
}

#[test]
fn budget_and_live_gate_precede_retained_side_entry_membership() {
    for budget in [0, 2, 3] {
        let (mut instance, mut context, mut bank, segments) = frontier_fixture(budget);
        instance
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1010), &[0x0f, 0x0b])
            .unwrap();
        let gates = [(GuestAddress(0x1010), 77)];
        let mut cache = CoreCache32::default();
        let owner = cache.insert(
            compile_with_gates(
                instance.memory.as_ref().unwrap(),
                GuestAddress(0x1008),
                frontier_options(2),
                &gates,
            )
            .unwrap(),
        );
        assert!(owner.entrypoints().contains(&GuestAddress(0x1010)));
        if budget == 0 {
            context.set_word(32, 0x1010);
        }
        let stop = run_with_frontier(
            &mut instance,
            &mut context,
            &mut bank,
            &segments,
            &gates,
            &|memory, pc| cache.frontier_contains(memory, pc),
            &mut ColdState::default(),
        );
        assert_eq!(
            stop,
            if budget == 3 {
                StopReason::Host
            } else {
                StopReason::Budget
            }
        );
        assert_eq!(context.state().eip, 0x1010);
        assert_eq!(context.retired(), if budget == 0 { 0 } else { 2 });
        assert_eq!(context.remaining(), if budget == 3 { 1 } else { 0 });
        assert_eq!(
            u32::from_le_bytes(context.bytes()[116..120].try_into().unwrap()),
            if budget == 3 { 77 } else { 0 },
        );
    }
}
