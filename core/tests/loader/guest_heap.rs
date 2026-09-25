use super::heap_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn heap_guest_calls_resume_with_the_same_state_and_budget() {
    let bytes = heap_executable::pe32();
    let mut whole = Process32::load(&bytes, 27).unwrap();
    let mut stepped = Process32::load(&bytes, 27).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 2);
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
    assert_eq!(stepped.memory.mapped_pages(), 25);
    assert_eq!(whole.cpu.register(Register32::Eax), 0);
    assert_eq!(whole.cpu.register(Register32::Esi), 0x2000_0000);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
