use super::onexit_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn registered_callbacks_remain_deferred_in_whole_and_stepped_guests() {
    let bytes = onexit_executable::pe32();
    let mut whole = Process32::load(&bytes, 25).unwrap();
    let mut stepped = Process32::load(&bytes, 25).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (8, 2));
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
    assert_eq!((instructions, calls), (8, 2));
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Eax), 0x0040_1090);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x0040_1080);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        let mut trace = [0; 4];
        process.memory.read(0x0040_2180, &mut trace).unwrap();
        assert_eq!(trace, [0; 4]);
    }
}
