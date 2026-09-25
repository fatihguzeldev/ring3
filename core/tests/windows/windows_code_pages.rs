use super::code_pages_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

#[test]
fn code_page_queries_are_zero_argument_identities_without_thread_access() {
    for (api, id) in [(0x7000_0088, 1252), (0x7000_008c, 437)] {
        for seed in [0, 42, u32::MAX] {
            let mut process = Process32::load(&code_pages_executable::pe32(), 32).unwrap();
            let pages = process.memory.mapped_pages();
            process
                .memory
                .write(0x1000_fffc, &0x0040_100e_u32.to_le_bytes())
                .unwrap();
            process
                .memory
                .write(0x7ffd_e034, &seed.to_le_bytes())
                .unwrap();
            process
                .memory
                .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
                .unwrap();
            process.cpu.eip = api;
            process.cpu.set_register(Register32::Esp, 0x1000_fffc);
            process.cpu.set_register(Register32::Eax, seed);
            process.cpu.set_register(Register32::Edi, seed);
            process.cpu.eflags = 0xced7;
            let mut expected = process.cpu;
            expected.eip = 0x0040_100e;
            expected.set_register(Register32::Esp, 0x1001_0000);
            expected.set_register(Register32::Eax, id);
            let result = process.run(1);
            assert_eq!(
                result.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
            assert_eq!((result.instructions, result.api_calls), (0, 1));
            assert_eq!(process.cpu, expected);
            assert_eq!(process.memory.mapped_pages(), pages);
            process
                .memory
                .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
                .unwrap();
            assert_eq!(process.last_error().unwrap(), seed);
        }
    }
}

#[test]
fn invalid_identity_frames_preserve_cpu_and_budget() {
    for api in [0x7000_0088, 0x7000_008c] {
        for stack in [0, 0x1001_0000, u32::MAX - 2] {
            let mut process = Process32::load(&code_pages_executable::pe32(), 32).unwrap();
            process.cpu.eip = api;
            process.cpu.set_register(Register32::Esp, stack);
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
}

#[test]
fn imported_identity_calls_resume_with_consistent_profile_values() {
    let bytes = code_pages_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let stack = whole.cpu.register(Register32::Esp);
    let result = whole.run(20);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (4, 2));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..20 {
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
    assert_eq!(whole.cpu.register(Register32::Eax), 437);
    assert_eq!(whole.cpu.register(Register32::Ebx), 1252);
    assert_eq!(whole.cpu.register(Register32::Esp), stack);
}
