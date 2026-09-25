use super::strdup_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn duplicate_modify_and_free_match_whole_and_single_step_execution() {
    let bytes = strdup_executable::pe32();
    let mut whole = Process32::load(&bytes, 26).unwrap();
    let mut stepped = Process32::load(&bytes, 26).unwrap();
    let pages = whole.memory.mapped_pages();
    let result = whole.run(50);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (11, 2));
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
    assert_eq!((instructions, calls), (11, 2));
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Eax), 0x2000_0000);
        assert_eq!(process.cpu.register(Register32::Ecx), 0x2000_0000);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x0063_6261);
        assert_eq!(process.cpu.register(Register32::Edx), 0x0063_622a);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.memory.mapped_pages(), pages);
        assert!(process.memory.read(0x2000_0000, &mut [0]).is_err());
        let mut source = [0; 4];
        process.memory.read(0x0040_2180, &mut source).unwrap();
        assert_eq!(&source, b"abc\0");
    }
}
