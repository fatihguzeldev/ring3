#[path = "support/hook_chain_cases.rs"]
mod hook_chain_cases;

use hook_chain_cases::{DATA, NEW, NEXT, OLD, call, forwarder, process, run};
use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

fn until_next(p: &mut Process32) {
    for _ in 0..1000 {
        if p.cpu.eip == NEXT {
            return;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    panic!("forwarding call not reached");
}

fn unchanged_stop(p: &mut Process32, cpu: Cpu32) -> ProcessStop {
    let result = p.run(1);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    result.reason
}

fn replace_code(p: &mut Process32, address: u32, bytes: &[u8]) {
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(address), bytes).unwrap();
    p.memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
}
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[test]
fn real_forwarding_preserves_arguments_results_and_creation_effects_across_budgets() {
    hook_chain_cases::verify();
}

#[test]
fn forwarding_requires_an_active_hook_and_does_not_leak_after_creation() {
    for completed in [false, true] {
        let mut p = process(0);
        if completed {
            run(&mut p, 1000);
        }
        p.cpu.eip = NEXT;
        p.cpu.set_register(Register32::Esp, 0x1000_c000);
        p.memory.write(0x1000_c000, &[0; 20]).unwrap();
        let cpu = p.cpu;
        assert_eq!(
            unchanged_stop(&mut p, cpu),
            ProcessStop::UnsupportedApi { address: NEXT }
        );
    }
}

#[test]
fn failed_callback_stack_write_retains_chain_position_for_retry() {
    let mut p = process(0);
    until_next(&mut p);
    let cpu = p.cpu;
    let page = u64::from(cpu.register(Register32::Esp) & !4095);
    p.memory.protect(page, 4096, Permissions::READ).unwrap();
    assert!(matches!(
        unchanged_stop(&mut p, cpu),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert!(matches!(
        unchanged_stop(&mut p, cpu),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory
        .protect(page, 4096, Permissions::READ_WRITE)
        .unwrap();
    run(&mut p, 1);
    assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn full_forwarding_input_is_read_before_callback_entry() {
    let mut p = process(0);
    until_next(&mut p);
    let original = p.cpu;
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let cpu = p.cpu;
    assert!(matches!(
        unchanged_stop(&mut p, cpu),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.cpu = original;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, original);
    run(&mut p, 1000);
    assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
}

#[test]
fn failed_child_return_retains_both_callback_contexts_for_retry() {
    let mut p = process(0);
    until_next(&mut p);
    assert_eq!(p.run(1).api_calls, 1);
    for _ in 0..100 {
        if p.cpu.eip == 0x7000_0ff8 {
            break;
        }
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.eip, 0x7000_0ff8);
    let cpu = p.cpu;
    let page = u64::from(cpu.register(Register32::Esp) & !4095);
    p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    assert!(matches!(
        unchanged_stop(&mut p, cpu),
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    p.memory
        .protect(page, 4096, Permissions::READ_WRITE)
        .unwrap();
    run(&mut p, 1);
    assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn chain_end_does_not_dereference_payload_or_error_fields() {
    let mut p = process(0);
    until_next(&mut p);
    call(&mut p, 0x7000_0250, &[0x7400_0004]);
    let stack = u64::from(p.cpu.register(Register32::Esp));
    p.memory
        .write(stack + 4, &u32::MAX.to_le_bytes().repeat(4))
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    let mut expected = p.cpu;
    let mut saved = [0; 4];
    p.memory.read(stack, &mut saved).unwrap();
    expected.eip = u32::from_le_bytes(saved);
    expected.set_register(Register32::Esp, u32::try_from(stack).unwrap() + 20);
    expected.set_register(Register32::Eax, 0);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu, expected);
    run(&mut p, 1000);
    let mut data = [0; 12];
    p.memory.read(DATA, &mut data).unwrap();
    assert_eq!(data, [0; 12]);
}

#[test]
fn all_argument_bits_and_result_bits_pass_through_unchanged() {
    let mut p = process(0);
    replace_code(
        &mut p,
        OLD,
        &[
            0x8b, 0x44, 0x24, 4, 0xa3, 0, 0x23, 0x40, 0, 0x8b, 0x44, 0x24, 8, 0xa3, 4, 0x23, 0x40,
            0, 0x8b, 0x44, 0x24, 12, 0xa3, 8, 0x23, 0x40, 0, 0xb8, 0xef, 0xcd, 0xab, 0x89, 0xc2,
            12, 0,
        ],
    );
    until_next(&mut p);
    let stack = p.cpu.register(Register32::Esp);
    let args = [0_u32, 0xffff_fff9, 0x8765_4321, 0xdead_beef];
    p.memory
        .write(u64::from(stack + 4), &args.map(u32::to_le_bytes).concat())
        .unwrap();
    assert_eq!(p.run(1).api_calls, 1);
    while p.cpu.eip != 0x7000_0ff8 {
        assert_eq!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!(p.cpu.register(Register32::Eax), 0x89ab_cdef);
    let mut data = [0; 12];
    p.memory.read(DATA, &mut data).unwrap();
    assert_eq!(
        data.as_slice(),
        args[1..]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
    );
    run(&mut p, 1000);
    assert_eq!(p.cpu.register(Register32::Ebx), 0);
}

#[test]
fn private_target_is_rejected_and_unmapped_target_faults_at_execution() {
    for target in [0x7000_0ff8, 0xdead_beef] {
        let mut p = process(0);
        for handle in [0x7400_0004, 0x7400_000c] {
            call(&mut p, 0x7000_0250, &[handle]);
        }
        let bad = call(&mut p, 0x7000_024c, &[5, target, 0, 1]);
        call(&mut p, 0x7000_024c, &[5, NEW, 0, 1]);
        until_next(&mut p);
        let cpu = p.cpu;
        if target == 0x7000_0ff8 {
            assert_eq!(
                unchanged_stop(&mut p, cpu),
                ProcessStop::UnsupportedApi { address: NEXT }
            );
            call(&mut p, 0x7000_0250, &[bad]);
            run(&mut p, 1000);
            assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
        } else {
            assert_eq!(p.run(1).api_calls, 1);
            assert_eq!(p.cpu.eip, target);
            let cpu = p.cpu;
            assert!(matches!(
                unchanged_stop(&mut p, cpu),
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
    }
}

#[test]
fn shared_depth_limit_bounds_forwarding_but_allows_terminal_call_at_limit() {
    for depth in [64, 65] {
        let mut p = process(0);
        replace_code(&mut p, OLD, &forwarder());
        for _ in 2..depth {
            call(&mut p, 0x7000_024c, &[5, NEW, 0, 1]);
        }
        let result = p.run(5000);
        if depth == 64 {
            assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
            assert_eq!(p.cpu.register(Register32::Ebx), 0x7500_0004);
        } else {
            assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: NEXT });
            let cpu = p.cpu;
            assert_eq!(unchanged_stop(&mut p, cpu), result.reason);
        }
    }
}
