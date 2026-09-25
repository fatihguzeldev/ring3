use super::crt_heap_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn crt_heap_guest_caller_cleanup_and_single_step_execution_match() {
    check_guest(&crt_heap_executable::pe32(false));
    check_guest(&crt_heap_executable::pe32(true));
}

fn check_guest(bytes: &[u8]) {
    let mut whole = Process32::load(bytes, 27).unwrap();
    let mut stepped = Process32::load(bytes, 27).unwrap();
    let stack = whole.cpu.register(Register32::Esp);
    let pages = whole.memory.mapped_pages();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 3);
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..100 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(
        (instructions, calls),
        (result.instructions, result.api_calls)
    );
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Esp), stack);
        assert_eq!(process.cpu.register(Register32::Eax), 123);
        assert_eq!(process.cpu.register(Register32::Ebx), 42);
        assert_eq!(process.memory.mapped_pages(), pages);
        assert!(
            process
                .memory
                .read(u64::from(process.cpu.register(Register32::Esi)), &mut [0])
                .is_err()
        );
    }
}
