#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_009c;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "USER32.dll", &["GetSystemMetrics"]),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, index: u32) -> Cpu32 {
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
        .write(u64::from(STACK + 4), &index.to_le_bytes())
        .unwrap();
    process.cpu
}

#[test]
fn metrics_return_the_fixed_guest_dimensions_without_other_state_changes() {
    let mut process = process();
    let pages = process.memory.mapped_pages();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    process
        .memory
        .protect(0x0040_0000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for (index, value) in [
        (0, 640),
        (1, 480),
        (4, 19),
        (16, 640),
        (17, 461),
        (2, 16),
        (3, 16),
        (9, 16),
        (10, 16),
        (20, 16),
        (21, 16),
        (11, 32),
        (12, 32),
        (49, 16),
        (50, 16),
    ] {
        let before = prepare(&mut process, index);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        let mut expected = before;
        expected.eip = 0x0040_1000;
        expected.set_register(Register32::Esp, STACK + 8);
        expected.set_register(Register32::Eax, value);
        assert_eq!(process.cpu, expected);
    }
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(process.last_error().unwrap(), 77);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn unsupported_metrics_and_faulting_frames_stop_without_side_effects() {
    let mut process = process();
    for index in [13, 42, 48, 51, 0x8000_0000, u32::MAX] {
        let before = prepare(&mut process, index);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    for stack in [0, 0x1000_fffc, u32::MAX - 3] {
        prepare(&mut process, 11);
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
    assert_eq!(process.last_error().unwrap(), 0);
}

#[path = "support/metrics_executable.rs"]
mod metrics_executable;

#[test]
fn icon_metrics_guest_matches_whole_and_single_step_execution() {
    check_guest(&metrics_executable::pe32(), [16, 32, 32, 16]);
}

#[test]
fn fullscreen_client_metrics_reserve_the_caption_from_the_guest_screen() {
    check_guest(&metrics_executable::fullscreen(), [461, 640, 480, 19]);
}

fn check_guest(bytes: &[u8], values: [u32; 4]) {
    let mut whole = Process32::load(bytes, 32).unwrap();
    let mut stepped = Process32::load(bytes, 32).unwrap();
    let result = whole.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (12, 4));
    let (mut instructions, mut calls) = (0, 0);
    for _ in 0..30 {
        let step = stepped.run(1);
        instructions += step.instructions;
        calls += step.api_calls;
        if step.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!((instructions, calls), (12, 4));
    assert_eq!(whole.cpu, stepped.cpu);
    assert_eq!(whole.cpu.register(Register32::Eax), values[0]);
    assert_eq!(whole.cpu.register(Register32::Ebx), values[1]);
    assert_eq!(whole.cpu.register(Register32::Ecx), values[2]);
    assert_eq!(whole.cpu.register(Register32::Edx), values[3]);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
