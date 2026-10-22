use super::*;
use crate::{
    cpu::x86::{State32, X87Format, X87Operation, ir::Operation},
    memory::{PageRange, Permissions},
};

fn memory(code: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(4).unwrap();
    for (pc, permissions) in [
        (0x1000, Permissions::ALL),
        (0x2000, Permissions::ALL),
        (0x3000, Permissions::READ_WRITE),
        (0x4000, Permissions::READ_WRITE),
    ] {
        memory
            .map_zeroed(PageRange::new(GuestAddress(pc), 1).unwrap(), permissions)
            .unwrap();
    }
    memory.write(GuestAddress(0x1000), code).unwrap();
    memory
}

fn context(budget: u32) -> Context32 {
    let mut context = Context32::new(State32 {
        registers: [5, 3, 7, 0x3000, 0x4800, 0, 0, 0],
        eip: 0x1000,
        eflags: 2,
    });
    context.set_word(44, budget);
    context.set_word(112, 1);
    context
}

fn stop(memory: &mut AddressSpace, context: &mut Context32, heat: &mut ColdState) -> StopReason {
    match run(memory, context, &SegmentContext::default(), &[], &[], heat) {
        ColdOutcome::Stop(stop) => stop,
        ColdOutcome::NeedNative(instruction) => {
            panic!("unexpected native operation at {:x}", instruction.pc)
        }
    }
}

fn word(context: &Context32, offset: usize) -> u32 {
    u32::from_le_bytes(context.bytes()[offset..offset + 4].try_into().unwrap())
}

#[test]
fn successful_width_arithmetic_preserves_the_unwritten_register_bits() {
    for (code, initial, expected) in [
        (&[0x83, 0xc0, 1][..], u32::MAX, 0),
        (&[0x66, 0x05, 1, 0][..], 0x1234_ffff, 0x1234_0000),
        (&[0x04, 1][..], 0x1234_56ff, 0x1234_5600),
    ] {
        let mut memory = memory(code);
        let mut context = context(1);
        context.set_word(0, initial);
        context.set_word(36, 0x402);
        assert_eq!(
            stop(&mut memory, &mut context, &mut ColdState::default()),
            StopReason::Budget
        );
        assert_eq!(
            context.state().registers,
            [expected, 3, 7, 0x3000, 0x4800, 0, 0, 0]
        );
        assert_eq!(context.state().eflags, 0x457);
        assert_eq!(context.state().eip, 0x1000 + code.len() as u32);
        assert_eq!((context.retired(), context.remaining()), (1, 0));
    }
}

#[test]
fn actual_calls_returns_and_memory_arithmetic_run_without_a_compiled_owner() {
    let mut memory = memory(&[0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b]);
    memory
        .write(
            GuestAddress(0x1040),
            &[0x83, 0xc0, 3, 0x31, 0xc2, 0x83, 0x43, 0x0c, 1, 0xc3],
        )
        .unwrap();
    let mut context = context(21);
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::Budget
    );
    assert_eq!(
        context.state(),
        State32 {
            registers: [14, 0, 10, 0x3000, 0x4800, 0, 0, 0],
            eip: 0x1008,
            eflags: 0x46,
        }
    );
    assert_eq!((context.retired(), context.remaining()), (21, 0));
    let mut value = [0; 4];
    memory.read(GuestAddress(0x300c), &mut value).unwrap();
    assert_eq!(value, 3_u32.to_le_bytes());
    memory.read(GuestAddress(0x47fc), &mut value).unwrap();
    assert_eq!(value, 0x1005_u32.to_le_bytes());
    assert_eq!(&context.bytes()[120..], &[0; 8]);
}

#[test]
fn budget_and_debug_precede_fetch_and_frontier_selection() {
    let mut memory = AddressSpace::new(1).unwrap();
    let mut context = context(0);
    context.set_word(32, 0xdead_0000);
    assert!(matches!(
        run(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &[],
            &[GuestAddress(0xdead_0000)],
            &mut ColdState::default()
        ),
        ColdOutcome::Stop(StopReason::Budget)
    ));
    assert_eq!(context.retired(), 0);
    context.set_word(36, 0x102);
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::Debug
    );
    assert_eq!(context.state().eip, 0xdead_0000);
}

#[test]
fn read_modify_fault_preserves_input_flags_registers_and_memory() {
    let mut memory = memory(&[0x01, 0x03]);
    memory
        .write(GuestAddress(0x3000), &0x7fff_ffff_u32.to_le_bytes())
        .unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let mut context = context(4);
    context.set_word(36, 0x8d7);
    let initial = context.state();
    let version = memory.transaction_version();
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::MemoryFault
    );
    assert_eq!(context.state(), initial);
    assert_eq!((context.retired(), context.remaining()), (0, 4));
    assert_eq!(
        (word(&context, 48), word(&context, 52), word(&context, 116)),
        (0x3000, 0x20204, 0x40e)
    );
    let mut value = [0; 4];
    memory.read(GuestAddress(0x3000), &mut value).unwrap();
    assert_eq!(value, 0x7fff_ffff_u32.to_le_bytes());
    assert_eq!(memory.transaction_version(), version);
}

#[test]
fn code_store_retires_once_and_stops_before_decoding_changed_bytes() {
    let mut memory = memory(&[0x89, 0x03, 0x0f, 0x0b]);
    let mut context = context(4);
    context.set_word(0, 0x1122_3344);
    context.set_word(12, 0x1000);
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::CodeInvalidated
    );
    assert_eq!(
        (context.state().eip, context.retired(), context.remaining()),
        (0x1002, 1, 3)
    );
    assert_eq!(context.state().eflags, 2);
    let mut bytes = [0; 4];
    memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
    assert_eq!(bytes, [0x44, 0x33, 0x22, 0x11]);
}

#[test]
fn live_gate_precedes_decoding_and_compiled_frontier() {
    let mut memory = memory(&[0x0f, 0x0b]);
    let mut context = context(7);
    assert!(matches!(
        run(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &[(GuestAddress(0x1000), 77)],
            &[GuestAddress(0x1000)],
            &mut ColdState::default()
        ),
        ColdOutcome::Stop(StopReason::Host)
    ));
    assert_eq!(
        (
            context.state().eip,
            context.retired(),
            context.remaining(),
            word(&context, 116)
        ),
        (0x1000, 0, 7, 77)
    );
}

#[test]
fn numerical_handoff_is_genuine_decoded_and_unretired_before_operand_access() {
    let mut memory = memory(&[0xd9, 0x03]);
    let mut context = context(9);
    context.set_word(12, 0xdead_0000);
    let before = *context.bytes();
    match run(
        &mut memory,
        &mut context,
        &SegmentContext::default(),
        &[],
        &[],
        &mut ColdState::default(),
    ) {
        ColdOutcome::NeedNative(instruction) => {
            assert_eq!((instruction.pc, instruction.next), (0x1000, 0x1002));
            assert!(matches!(
                instruction.operation,
                Operation::X87 {
                    operation: X87Operation::Load(X87Format::Float32),
                    ..
                }
            ));
        }
        outcome => panic!("expected native handoff: {outcome:?}"),
    }
    assert_eq!(*context.bytes(), before);
}

#[test]
fn an_unimplemented_cold_operation_requests_jit_before_any_effect() {
    let mut memory = memory(&[0xd1, 0xc0]);
    let mut context = context(3);
    context.set_word(36, 0x8d7);
    let before = *context.bytes();
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::NeedCode
    );
    assert_eq!(*context.bytes(), before);
}

#[test]
fn a_jit_fallback_keeps_the_already_committed_cold_prefix() {
    let mut memory = memory(&[0x40, 0xd1, 0xc0]);
    let mut context = context(4);
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::NeedCode
    );
    assert_eq!(
        (
            context.state().eip,
            context.state().registers[0],
            context.state().eflags,
            context.retired(),
            context.remaining()
        ),
        (0x1001, 6, 6, 1, 3)
    );
}

#[test]
fn call_esp_captures_its_target_before_the_stack_pointer_changes() {
    let mut memory = memory(&[0xff, 0xd4]);
    let mut context = context(1);
    assert_eq!(
        stop(&mut memory, &mut context, &mut ColdState::default()),
        StopReason::Budget
    );
    assert_eq!(
        (
            context.state().eip,
            context.state().registers[4],
            context.state().eflags,
            context.retired(),
            context.remaining()
        ),
        (0x4800, 0x47fc, 2, 1, 0)
    );
    let mut bytes = [0; 4];
    memory.read(GuestAddress(0x47fc), &mut bytes).unwrap();
    assert_eq!(bytes, 0x1002_u32.to_le_bytes());
}

#[test]
fn compiled_frontier_requests_a_closed_handoff_without_executing_the_target() {
    let mut memory = memory(&[0xeb, 0x3e]);
    memory.write(GuestAddress(0x1040), &[0x40]).unwrap();
    let mut context = context(4);
    assert!(matches!(
        run(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &[],
            &[GuestAddress(0x1040)],
            &mut ColdState::default()
        ),
        ColdOutcome::Stop(StopReason::NeedCode)
    ));
    assert_eq!(
        (
            context.state().eip,
            context.state().registers[0],
            context.retired(),
            context.remaining()
        ),
        (0x1040, 5, 1, 3)
    );
}

#[test]
fn retained_frontier_handoffs_preserve_completed_cold_reuse() {
    let mut memory = memory(&[0x40, 0xeb, 0x3d]);
    memory.write(GuestAddress(0x1040), &[0xeb, 0xbe]).unwrap();
    let mut context = context(8);
    let mut cold = ColdState::with_threshold(2);
    for (retired, eax, flags) in [(2, 6, 6), (5, 7, 2)] {
        context.set_word(44, 8);
        assert!(matches!(
            run(
                &mut memory,
                &mut context,
                &SegmentContext::default(),
                &[],
                &[GuestAddress(0x1040)],
                &mut cold
            ),
            ColdOutcome::Stop(StopReason::NeedCode)
        ));
        assert_eq!(
            (
                context.state().eip,
                context.state().registers[0],
                context.state().eflags,
                context.retired(),
                context.remaining(),
            ),
            (0x1040, eax, flags, retired, 6)
        );
        // the retained target's completed jump returns to the interpreted caller.
        context.set_word(32, 0x1000);
        context.set_word(40, retired + 1);
        context.set_word(44, 5);
    }
    context.set_word(44, 8);
    let before = *context.bytes();
    assert!(matches!(
        run(
            &mut memory,
            &mut context,
            &SegmentContext::default(),
            &[],
            &[GuestAddress(0x1040)],
            &mut cold
        ),
        ColdOutcome::Stop(StopReason::NeedCode)
    ));
    assert_eq!(
        *context.bytes(),
        before,
        "hot request precedes the third INC"
    );
}

#[test]
fn heat_uses_completed_retirements_and_a_jit_request_requires_new_reuse() {
    let mut memory = memory(&[0x40, 0xeb, 0xfd]);
    let mut context = context(4);
    let mut heat = ColdState::with_threshold(2);
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::Budget
    );
    assert_eq!((context.retired(), context.state().registers[0]), (4, 7));
    context.set_word(44, 10);
    let before = *context.bytes();
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::NeedCode
    );
    assert_eq!(*context.bytes(), before);
    context.set_word(44, 2);
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::NeedCode
    );
    assert_eq!(context.state().eip, 0x1001);
    assert_eq!(context.retired(), 5);
    assert_eq!(context.remaining(), 1);
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::Budget
    );
    assert_eq!(
        (
            context.retired(),
            context.state().registers[0],
            context.remaining()
        ),
        (6, 8, 0)
    );
}

#[test]
fn colliding_heat_sites_retain_independent_completed_reuse() {
    let memory = memory(&[0x90]);
    let mut heat = ColdHeat::new(2);
    let sites = [0x1000, 0x3004];
    assert_eq!(ColdHeat::index(sites[0]), ColdHeat::index(sites[1]));
    for _ in 0..2 {
        for pc in sites {
            assert!(!heat.hot(&memory, pc));
            heat.retired(&memory, pc);
        }
    }
    for pc in sites {
        assert!(heat.hot(&memory, pc));
    }
    heat.requested(sites[0]);
    assert!(!heat.hot(&memory, sites[0]));
    assert!(heat.hot(&memory, sites[1]));
    heat.retired(&memory, sites[0]);
    assert!(!heat.hot(&memory, sites[0]));
    heat.retired(&memory, sites[0]);
    assert!(heat.hot(&memory, sites[0]));
}

#[test]
fn compiled_instruction_coverage_consumes_only_its_completed_reuse() {
    let memory = memory(&[0xb8, 1, 0, 0, 0, 0x90]);
    let mut cold = ColdState::default();
    let admitted = [0x1000, 0x1005];
    let unrelated = [0x1001, 0x3004, 0x2000];
    assert_eq!(ColdHeat::index(admitted[0]), ColdHeat::index(unrelated[1]));
    for _ in 0..HOT_VISITS {
        for pc in admitted.into_iter().chain(unrelated) {
            cold.retired(&memory, pc);
        }
    }
    let before = cold.diagnostics_snapshot();
    let generation = cold.heat.generation;
    cold.forget_compiled(&memory, admitted);
    assert_eq!(cold.heat.generation, generation);
    assert_eq!(&cold.diagnostics_snapshot()[..9], &before[..9]);
    for pc in admitted {
        assert!(!cold.heat.hot(&memory, pc));
    }
    for pc in unrelated {
        assert!(
            cold.heat.hot(&memory, pc),
            "operand-byte, colliding and unrelated PCs retain independent reuse"
        );
    }
    for _ in 0..HOT_VISITS - 1 {
        for pc in admitted {
            cold.retired(&memory, pc);
        }
    }
    for pc in admitted {
        assert!(!cold.heat.hot(&memory, pc));
        cold.retired(&memory, pc);
        assert!(cold.heat.hot(&memory, pc));
    }
}

#[test]
fn compiled_coverage_for_another_memory_does_not_rebind_or_consume_heat() {
    let memory = memory(&[0x90]);
    let other_memory = AddressSpace::new(1).unwrap();
    let mut cold = ColdState::default();
    for _ in 0..HOT_VISITS {
        cold.retired(&memory, 0x1000);
    }
    let before = cold.diagnostics_snapshot();
    let generation = cold.heat.generation;
    cold.forget_compiled(&other_memory, [0x1000]);
    assert_eq!(cold.heat.generation, generation);
    assert_eq!(cold.heat.memory, memory.identity());
    assert_eq!(cold.diagnostics_snapshot(), before);
    assert!(cold.heat.hot(&memory, 0x1000));
    assert!(!cold.heat.hot(&other_memory, 0x1000));
    assert_eq!(cold.heat.memory, other_memory.identity());
    assert_eq!(cold.diagnostics_snapshot()[1], before[1] + 1);
}

#[test]
fn repeated_working_set_qualifies_within_bounded_heat_storage() {
    let memory = memory(&[0x90]);
    let mut cold = ColdState::default();
    assert!(std::mem::size_of_val(&*cold.heat.rows) <= 512 * 1024);
    let sites = 0x0100_0000..0x0100_4000;
    for _ in 0..HOT_VISITS {
        for pc in sites.clone() {
            cold.retired(&memory, pc);
        }
    }
    for pc in sites {
        assert!(cold.heat.hot(&memory, pc), "lost completed reuse at {pc:x}");
    }
    let diagnostics = cold.diagnostics_snapshot();
    assert_eq!(
        diagnostics[3], 0,
        "this working set needs no occupied victim"
    );
    assert_eq!(&diagnostics[9..], &[0, 0, 16_384, 64]);
}

#[test]
fn unrelated_requests_cannot_starve_completed_reuse() {
    let mut memory = memory(&[0x40, 0x0f, 0x0b]);
    let mut context = context(2);
    let mut cold = ColdState::default();
    for retired in 1..=HOT_VISITS {
        context.set_word(32, 0x1000);
        context.set_word(44, 2);
        assert_eq!(
            stop(&mut memory, &mut context, &mut cold),
            StopReason::NeedCode
        );
        assert_eq!(context.state().eip, 0x1001);
        assert_eq!(context.state().registers[0], 5 + retired);
        assert_eq!(context.retired(), retired);
        assert_eq!(context.remaining(), 1);
    }
    context.set_word(32, 0x1000);
    context.set_word(44, 1);
    let before = *context.bytes();
    assert_eq!(
        stop(&mut memory, &mut context, &mut cold),
        StopReason::NeedCode
    );
    assert_eq!(
        *context.bytes(),
        before,
        "hot request precedes the 65th INC"
    );
}

#[test]
fn memory_writes_preserve_other_reuse_and_requested_pc_requalifies() {
    let mut memory = memory(&[0x40, 0xeb, 0xfd]);
    let mut context = context(4);
    let mut heat = ColdState::with_threshold(2);
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::Budget
    );
    memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
    memory.write(GuestAddress(0x3000), &[0x90]).unwrap();
    context.set_word(44, 4);
    let before = *context.bytes();
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::NeedCode
    );
    assert_eq!(*context.bytes(), before);
    memory
        .write(GuestAddress(0x1000), &[0x48, 0xeb, 0xfd])
        .unwrap();
    context.set_word(44, 1);
    assert_eq!(
        stop(&mut memory, &mut context, &mut heat),
        StopReason::Budget
    );
    assert_eq!(
        context.state(),
        State32 {
            registers: [6, 3, 7, 0x3000, 0x4800, 0, 0, 0],
            eip: 0x1001,
            eflags: 6,
        }
    );
    assert_eq!((context.retired(), context.remaining()), (5, 0));
}

#[test]
fn inactive_or_compiled_context_is_rejected_without_changing_it() {
    let mut memory = memory(&[0x40]);
    for (active, identity) in [(0, 0_u64), (1, 9)] {
        let mut context = context(1);
        context.set_word(112, active);
        context.bytes_mut()[120..].copy_from_slice(&identity.to_le_bytes());
        let before = *context.bytes();
        assert_eq!(
            stop(&mut memory, &mut context, &mut ColdState::default()),
            StopReason::Infrastructure
        );
        assert_eq!(*context.bytes(), before);
    }
}

#[test]
fn cold_frequency_hint_survives_unrelated_rw_unmap() {
    let mut memory = memory(&[0x40, 0xeb, 0xfd]);
    let mut context = context(64);
    let mut cold = ColdState::default();
    assert_eq!(
        stop(&mut memory, &mut context, &mut cold),
        StopReason::Budget
    );
    assert_eq!((context.retired(), context.state().registers[0]), (64, 37));
    memory
        .unmap(PageRange::new(GuestAddress(0x3000), 1).unwrap())
        .unwrap();
    context.set_word(44, 64);
    assert_eq!(
        stop(&mut memory, &mut context, &mut cold),
        StopReason::Budget
    );
    assert_eq!(
        context.state(),
        State32 {
            registers: [69, 3, 7, 0x3000, 0x4800, 0, 0, 0],
            eip: 0x1000,
            eflags: 2,
        }
    );
    assert_eq!((context.retired(), context.remaining()), (128, 0));
    context.set_word(44, 1);
    let before = *context.bytes();
    assert_eq!(
        stop(&mut memory, &mut context, &mut cold),
        StopReason::NeedCode
    );
    assert_eq!(*context.bytes(), before);
}

#[test]
fn cold_frequency_hint_never_bypasses_changed_execute_permission() {
    let mut memory = memory(&[0x40, 0xeb, 0xfd]);
    let mut context = context(128);
    let mut cold = ColdState::default();
    assert_eq!(
        stop(&mut memory, &mut context, &mut cold),
        StopReason::Budget
    );
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    context.set_word(44, 1);
    let mut expected = context.clone();
    expected.set_word(48, 0x1000);
    expected.set_word(52, 0x20401);
    expected.set_word(116, 0x10e);
    assert_eq!(
        stop(&mut memory, &mut context, &mut cold),
        StopReason::MemoryFault
    );
    assert_eq!(*context.bytes(), *expected.bytes());
}

#[test]
fn cold_frequency_hint_is_not_shared_between_memory_owners() {
    let mut first = memory(&[0x40, 0xeb, 0xfd]);
    let mut second = memory(&[0x40, 0xeb, 0xfd]);
    assert_ne!(first.identity(), second.identity());
    assert_eq!(first.code_source_version(), second.code_source_version());
    let mut context = context(128);
    let mut cold = ColdState::default();
    assert_eq!(
        stop(&mut first, &mut context, &mut cold),
        StopReason::Budget
    );
    context.set_word(44, 1);
    assert_eq!(
        stop(&mut second, &mut context, &mut cold),
        StopReason::Budget
    );
    assert_eq!(
        context.state(),
        State32 {
            registers: [70, 3, 7, 0x3000, 0x4800, 0, 0, 0],
            eip: 0x1001,
            eflags: 2,
        }
    );
    assert_eq!((context.retired(), context.remaining()), (129, 0));
}
