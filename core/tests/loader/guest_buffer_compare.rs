use super::buffer_compare_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn buffer_comparison_cdecl_calls_match_whole_and_single_step_execution() {
    let bytes = buffer_compare_executable::pe32();
    let mut whole = Process32::load(&bytes, 25).unwrap();
    let mut stepped = Process32::load(&bytes, 25).unwrap();
    let result = whole.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 2));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..50 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!((instructions, calls), (12, 2));
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(process.cpu.register(Register32::Ebx), 1);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 3];
        process.memory.read(0x0040_2180, &mut bytes).unwrap();
        assert_eq!(&bytes, b"a\0\x80");
    }
}
