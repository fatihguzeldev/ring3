#[path = "support/crt_code_page_executable.rs"]
mod crt_code_page_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn code_page_selection_and_character_calls_match_whole_and_stepped_guests() {
    let bytes = crt_code_page_executable::pe32();
    let mut whole = Process32::load(&bytes, 25).unwrap();
    let mut stepped = Process32::load(&bytes, 25).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (13, 3));
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
    assert_eq!((instructions, calls), (13, 3));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 0x0040_2182);
    assert_eq!(whole.cpu.register(Register32::Ebx), 0);
    assert_eq!(whole.cpu.register(Register32::Esi), 0x0040_2181);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
