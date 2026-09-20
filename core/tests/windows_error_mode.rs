#[path = "support/error_mode_executable.rs"]
mod error_mode_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn documented_modes_return_previous_state_and_ignore_x86_alignment_bit() {
    for bits in 0..16_u32 {
        let mode = (bits & 7) | ((bits & 8) << 12);
        let mut process = Process32::load(&error_mode_executable::pe32(mode), 32).unwrap();
        process
            .memory
            .write(0x7ffd_e034, &99_u32.to_le_bytes())
            .unwrap();
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 4);
        assert_eq!(process.cpu.register(Register32::Ebx), 0);
        assert_eq!(process.cpu.register(Register32::Esi), mode & 0x8003);
        assert_eq!(process.cpu.register(Register32::Edi), mode & 0x8003);
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.last_error().unwrap(), 99);
    }
}

#[test]
fn modes_are_process_local_and_single_steps_match_whole_execution() {
    let bytes = error_mode_executable::pe32(0x8003);
    let mut process = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    assert_eq!(process.run(3).api_calls, 1);
    let mut separate = Process32::load(&error_mode_executable::pe32(0), 32).unwrap();
    assert_eq!(
        separate.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(separate.cpu.register(Register32::Ebx), 0);
    let result = process.run(100);
    let mut calls = 0;
    let mut instructions = 0;
    for _ in 0..100 {
        let step = stepped.run(1);
        calls += step.api_calls;
        instructions += step.instructions;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            break;
        }
    }
    assert_eq!(
        (instructions, calls),
        (result.instructions + 2, result.api_calls + 1)
    );
    assert_eq!(stepped.cpu, process.cpu);
}

#[test]
fn invalid_mode_and_faulty_frame_preserve_previous_mode_and_cpu() {
    for (mode, fault) in [(0x8008, false), (0x8001, true)] {
        let mut process = Process32::load(&error_mode_executable::pe32(mode), 32).unwrap();
        process.run(2);
        let stack = process.cpu.register(Register32::Esp);
        if fault {
            process.cpu.set_register(Register32::Esp, u32::MAX - 3);
        }
        let before = process.cpu;
        let result = process.run(1);
        if fault {
            assert!(matches!(
                result.reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        } else {
            assert!(matches!(result.reason, ProcessStop::UnsupportedApi { .. }));
        }
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.last_error().unwrap(), 0);
        process.cpu.set_register(Register32::Esp, stack);
        process
            .memory
            .write(u64::from(stack + 4), &0_u32.to_le_bytes())
            .unwrap();
        assert_eq!(process.run(1).api_calls, 1);
        assert_eq!(process.cpu.register(Register32::Eax), 0);
    }
}
