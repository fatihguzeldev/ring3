#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/memset_executable.rs"]
mod memset_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn memset_guest_caller_cleanup_and_single_step_execution_match() {
    let bytes = memset_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let stack = whole.cpu.register(Register32::Esp);
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
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
        assert_eq!(process.cpu.register(Register32::Eax), 0x0040_2180);
        assert_eq!(process.cpu.register(Register32::Ebx), 0xa5a5_a5a5);
        let mut data = [0; 8];
        process.memory.read(0x0040_2180, &mut data).unwrap();
        assert_eq!(data, [0xa5; 8]);
    }
}
