#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/initializer_executable.rs"]
mod initializer_executable;

use ring3_core::execution::{
    LoadError, MemoryError, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn result(process: &Process32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(0x0040_21c0, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn initializers_execute_in_order_with_lazy_table_reads_and_nested_api_calls() {
    for seed in [0_u32, 13, u32::MAX] {
        let mut process = Process32::load(&initializer_executable::pe32(seed), 32).unwrap();
        for (reg, value) in [
            (Register32::Ebx, 11),
            (Register32::Ebp, 12),
            (Register32::Esi, 13),
            (Register32::Edi, 14),
        ] {
            process.cpu.set_register(reg, value);
        }
        let stack = process.cpu.register(Register32::Esp);
        let run = process.run(500);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(run.api_calls, 1);
        assert_eq!(result(&process), seed.wrapping_add(8).wrapping_mul(2));
        assert_eq!(process.cpu.register(Register32::Esp), stack);
        for (reg, value) in [
            (Register32::Ebx, 11),
            (Register32::Ebp, 12),
            (Register32::Esi, 13),
            (Register32::Edi, 14),
        ] {
            assert_eq!(process.cpu.register(reg), value);
        }
    }
}

#[test]
fn single_step_budgets_resume_inside_initializers_and_nested_calls() {
    let bytes = initializer_executable::pe32(13);
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let run = whole.run(500);
    let mut process = Process32::load(&bytes, 32).unwrap();
    let mut instructions = 0;
    let mut calls = 0;
    for _ in 0..500 {
        let step = process.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        assert_eq!(step.instructions + step.api_calls, 1);
        if step.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
            break;
        }
        assert_eq!(
            step.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
    }
    assert_eq!((instructions, calls), (run.instructions, run.api_calls));
    assert_eq!(process.cpu, whole.cpu);
    assert_eq!(result(&process), 42);
}

#[test]
fn empty_and_reversed_ranges_do_not_read_the_table() {
    for (start, end) in [(0xdead_0000_u32, 0xdead_0000_u32), (u32::MAX, 1)] {
        let mut bytes = initializer_executable::pe32(13);
        bytes[513..517].copy_from_slice(&end.to_le_bytes());
        bytes[518..522].copy_from_slice(&start.to_le_bytes());
        let mut process = Process32::load(&bytes, 32).unwrap();
        assert_eq!(
            process.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(result(&process), 0);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn invalid_tables_targets_and_stack_permissions_stop_explicitly() {
    let mut bytes = initializer_executable::pe32(13);
    bytes[513..517].copy_from_slice(&0xdead_0004_u32.to_le_bytes());
    bytes[518..522].copy_from_slice(&0xdead_0000_u32.to_le_bytes());
    let mut process = Process32::load(&bytes, 32).unwrap();
    assert!(matches!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result(&process), 0);
    let mut process = Process32::load(&initializer_executable::pe32(13), 32).unwrap();
    process
        .memory
        .write(0x0040_2180, &0x5000_0000_u32.to_le_bytes())
        .unwrap();
    assert!(matches!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu.eip, 0x5000_0000);
    let mut process = Process32::load(&initializer_executable::pe32(13), 32).unwrap();
    process
        .memory
        .protect(0x1000_f000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = process.cpu;
    assert!(matches!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
}

#[test]
fn guest_routine_is_read_only_and_counts_against_page_cap() {
    let bytes = initializer_executable::pe32(13);
    assert!(matches!(
        Process32::load(&bytes, 24),
        Err(LoadError::Memory(MemoryError::PageLimitExceeded))
    ));
    let mut process = Process32::load(&bytes, 25).unwrap();
    assert!(process.memory.fetch(0x7000_3000, &mut [0]).is_ok());
    assert!(process.memory.write(0x7000_3000, &[0xcc]).is_err());
}
