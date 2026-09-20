#[path = "support/critical_section_executable.rs"]
mod critical_section_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn recursive_guest_lifecycle_resumes_with_the_same_state_and_budget() {
    let bytes = critical_section_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 8);
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
    assert_eq!(stepped.cpu, whole.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 1);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
    for process in [&whole, &stepped] {
        let mut bytes = [1; 24];
        process.memory.read(0x0040_2180, &mut bytes).unwrap();
        assert_eq!(bytes, [0; 24]);
    }
}
