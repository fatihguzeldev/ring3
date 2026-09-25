use super::imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

#[test]
fn version_identity_has_zero_arguments_and_preserves_other_state() {
    let bytes = imported_executable::pe32(
        &[0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc],
        "KERNEL32.dll",
        &["GetVersion"],
    );
    for seed in [0, 42, u32::MAX] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        process.cpu.set_register(Register32::Eax, seed);
        process.cpu.set_register(Register32::Ebx, seed);
        process.cpu.eflags = 0xad7;
        process
            .memory
            .write(0x7ffd_e034, &seed.to_le_bytes())
            .unwrap();
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        assert_eq!(process.run(1).instructions, 1);
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        let mut expected = before;
        expected.eip = 0x0040_1006;
        expected.set_register(Register32::Esp, 0x1001_0000);
        expected.set_register(Register32::Eax, 0x0a28_0105);
        assert_eq!(process.cpu, expected);
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        assert_eq!(process.last_error().unwrap(), seed);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
    }
}

#[test]
fn faulting_version_return_frame_leaves_cpu_and_budget_unchanged() {
    let bytes = imported_executable::pe32(&[0xcc], "kernel32.dll", &["GetVersion"]);
    for stack in [0, 0x1001_0000, u32::MAX - 2] {
        let mut process = Process32::load(&bytes, 32).unwrap();
        process.cpu.eip = 0x7000_0030;
        process.cpu.set_register(Register32::Esp, stack);
        process.cpu.set_register(Register32::Eax, 42);
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
}
