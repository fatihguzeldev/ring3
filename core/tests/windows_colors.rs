#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_00ac;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "USER32.dll", &["GetSysColor"]),
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
fn colors_follow_the_classic_profile_and_preserve_other_state() {
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
    let rgb = [
        (212, 208, 200),
        (58, 110, 165),
        (10, 36, 106),
        (128, 128, 128),
        (212, 208, 200),
        (255, 255, 255),
        (0, 0, 0),
        (0, 0, 0),
        (0, 0, 0),
        (255, 255, 255),
        (212, 208, 200),
        (212, 208, 200),
        (128, 128, 128),
        (10, 36, 106),
        (255, 255, 255),
        (212, 208, 200),
        (128, 128, 128),
        (128, 128, 128),
        (0, 0, 0),
        (212, 208, 200),
        (255, 255, 255),
        (64, 64, 64),
        (212, 208, 200),
        (0, 0, 0),
        (255, 255, 225),
        (0, 0, 0),
        (0, 0, 200),
        (166, 202, 240),
        (192, 192, 192),
        (10, 36, 106),
        (212, 208, 200),
    ];
    for (index, (red, green, blue)) in rgb.into_iter().enumerate() {
        if index == 25 {
            continue;
        }
        let before = prepare(&mut process, u32::try_from(index).unwrap());
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        let mut expected = before;
        expected.eip = 0x0040_1000;
        expected.set_register(Register32::Esp, STACK + 8);
        expected.set_register(Register32::Eax, red + green * 256 + blue * 65536);
        assert_eq!(process.cpu, expected);
    }
    for index in [31, 100, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
        let mut expected = prepare(&mut process, index);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        expected.eip = 0x0040_1000;
        expected.set_register(Register32::Esp, STACK + 8);
        expected.set_register(Register32::Eax, 0);
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
fn undocumented_index_and_bad_frames_stop_without_side_effects() {
    let mut process = process();
    let before = prepare(&mut process, 25);
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    for stack in [0, 0x1000_fffc, u32::MAX - 3] {
        prepare(&mut process, 15);
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

#[path = "support/colors_executable.rs"]
mod colors_executable;

#[test]
fn color_queries_match_whole_and_single_step_execution() {
    let bytes = colors_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
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
    assert_eq!(whole.cpu.register(Register32::Eax), 0);
    assert_eq!(whole.cpu.register(Register32::Ebx), 0x00c8_d0d4);
    assert_eq!(whole.cpu.register(Register32::Ecx), 0x00a5_6e3a);
    assert_eq!(whole.cpu.register(Register32::Edx), 0x00ff_ffff);
    assert_eq!(whole.cpu.register(Register32::Esp), 0x1001_0000);
}
