#[path = "support/exception_frame_executable.rs"]
mod exception_frame_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn nested_guest_frames_link_and_unlink_with_single_step_parity() {
    let bytes = exception_frame_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    for process in [&mut whole, &mut stepped] {
        process.cpu.eflags = 0xced7;
        process.cpu.set_register(Register32::Ebp, 0x8765_4321);
    }
    let stack = whole.cpu.register(Register32::Esp);
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
    assert_eq!(whole.cpu, stepped.cpu);
    for process in [&whole, &stepped] {
        assert_eq!(process.cpu.register(Register32::Esp), stack);
        assert_eq!(process.cpu.register(Register32::Ebp), 0x8765_4321);
        assert_eq!(process.cpu.register(Register32::Eax), 42);
        assert_eq!(process.cpu.eflags, 0xced7);
        let mut chain = [0; 4];
        process.memory.read(0x7ffd_e000, &mut chain).unwrap();
        assert_eq!(chain, [0xff; 4]);
        let mut data = [0; 24];
        process.memory.read(0x0040_2180, &mut data).unwrap();
        let expected = [
            stack - 20,
            stack - 20,
            stack - 40,
            stack - 20,
            0x2222_2222,
            u32::MAX,
        ];
        for (bytes, value) in data.chunks_exact(4).zip(expected) {
            assert_eq!(bytes, value.to_le_bytes());
        }
    }
}
