use super::imported_executable;
use super::process_version_executable;

use ring3_core::execution::{
    Cpu32, GuestModule, PAGE_SIZE, Permissions, Process32, ProcessOptions, ProcessStop, Register32,
    StopReason,
};

const API: u32 = 0x7000_0098;
const STACK: u32 = 0x1000_ff00;

fn prepare(process: &mut Process32, id: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(STACK + 4), &id.to_le_bytes())
        .unwrap();
    process.cpu
}

#[test]
fn process_version_uses_the_original_exe_subsystem_fields_and_preserves_state() {
    for (major, minor, result) in [
        (0, 0, 0_u32),
        (5, 1, 0x0005_0001),
        (0x1234, 0xabcd, 0x1234_abcd),
        (0xffff, 0xffff, u32::MAX),
    ] {
        let mut process =
            Process32::load(&process_version_executable::pe32(major, minor), 32).unwrap();
        process
            .memory
            .protect(0x0040_0000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        process
            .memory
            .write(0x0040_00c8, &0x0102_0304_u32.to_le_bytes())
            .unwrap();
        process
            .memory
            .protect(0x0040_0000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        process
            .memory
            .write(0x7ffd_e034, &77_u32.to_le_bytes())
            .unwrap();
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        let before = prepare(&mut process, 0);
        let outcome = process.run(1);
        assert_eq!(
            outcome.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((outcome.instructions, outcome.api_calls), (0, 1));
        let mut expected = before;
        expected.eip = 0x0040_1000;
        expected.set_register(Register32::Esp, STACK + 8);
        expected.set_register(Register32::Eax, result);
        assert_eq!(process.cpu, expected);
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        assert_eq!(process.last_error().unwrap(), 77);
    }
}

#[test]
fn unsupported_process_identifiers_and_frame_faults_do_not_mutate_state() {
    let mut process = Process32::load(&process_version_executable::pe32(5, 1), 32).unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    for id in [1, 42, u32::MAX] {
        let before = prepare(&mut process, id);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        assert_eq!(process.last_error().unwrap(), 77);
    }
    for stack in [0, 0x1000_fffc, u32::MAX - 3] {
        prepare(&mut process, 0);
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

#[test]
fn dll_attach_queries_the_main_exe_version_instead_of_its_own_header() {
    let mut dll = imported_executable::pe32(
        &[
            0x6a, 0, 0xff, 0x15, 0x60, 0x20, 0x60, 0, 0xa3, 0x80, 0x21, 0x60, 0, 0xb8, 1, 0, 0, 0,
            0xc2, 12, 0,
        ],
        "KERNEL32.dll",
        &["GetProcessVersion"],
    );
    dll[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    dll[0xb4..0xb8].copy_from_slice(&0x0060_0000_u32.to_le_bytes());
    dll[0xc8..0xcc].copy_from_slice(&0x0016_000b_u32.to_le_bytes());
    let modules = [GuestModule {
        name: "version.dll",
        bytes: &dll,
    }];
    let mut process = Process32::load_with_options(
        &process_version_executable::pe32(5, 1),
        40,
        ProcessOptions {
            modules: &modules,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    let mut value = [0; 4];
    process.memory.read(0x0060_2180, &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 0x0005_0001);
    assert_eq!(process.cpu.register(Register32::Eax), 0x0005_0001);
}

#[test]
fn process_version_guest_matches_whole_and_single_step_execution() {
    let bytes = process_version_executable::pe32(9, 2);
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(10);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (3, 1));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..10 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!((instructions, calls), (3, 1));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), 0x0009_0002);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
