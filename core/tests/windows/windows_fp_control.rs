use super::fp_control_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn crt_and_guest_control_instructions_share_masked_state() {
    for (initial, value, mask, expected_word, expected_flags) in [
        (0x027f_u16, 0, 0, 0x027f, 0x0009_001f),
        (0x027f, 0x0002_0300, 0x0003_0300, 0x0c7f, 0x000a_031f),
        (0x027f, 0, 0x0003_0000, 0x037f, 0x0008_001f),
        (0x027f, 0, 0x0008_001f, 0x0242, 0x0009_0000),
        (0x027d, 0x0008_001f, 0x0008_001f, 0x027d, 0x0001_001f),
        (0x027f, 0x0004_0100, 0x0004_0300, 0x167f, 0x000d_011f),
        (0x027f, 0xffff_ffff, 0xfff8_fce0, 0x027f, 0x0009_001f),
        (0x027f, 0x200, 0x300, 0x0a7f, 0x0009_021f),
        (0x837f, 0x0001_0000, 0x0003_0000, 0x827f, 0x0009_001f),
    ] {
        let bytes = fp_control_executable::pe32(initial, value, mask);
        let mut process = Process32::load(&bytes, 32).unwrap();
        let stack = process.cpu.register(Register32::Esp);
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 2);
        assert_eq!(process.cpu.x87_control_word(), expected_word);
        assert_eq!(process.cpu.register(Register32::Eax), expected_flags);
        assert_eq!(process.cpu.register(Register32::Ebx), 0x0009_001f);
        assert_eq!(process.cpu.register(Register32::Esp), stack);
        let mut saved = [0; 2];
        process.memory.read(0x0040_2300, &mut saved).unwrap();
        assert_eq!(saved, 0x027f_u16.to_le_bytes());
        process.memory.read(0x0040_2308, &mut saved).unwrap();
        assert_eq!(saved, expected_word.to_le_bytes());
    }
}

#[test]
fn each_exception_mask_uses_the_windows_flag_encoding() {
    for (flag, bit) in [(0x10, 1_u16), (8, 4), (4, 8), (2, 0x10), (1, 0x20)] {
        let bytes = fp_control_executable::pe32(0x027f, 0, flag);
        let mut process = Process32::load(&bytes, 32).unwrap();
        assert_eq!(
            process.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(process.cpu.x87_control_word(), 0x027f & !bit);
        assert_eq!(process.cpu.register(Register32::Eax), 0x0009_001f & !flag);
    }
}

#[test]
fn control_frame_fault_preserves_cpu_state() {
    let bytes = fp_control_executable::pe32(0x007f, 0, 0);
    let mut process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(process.run(4).instructions, 4);
    process.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert_eq!(process.cpu, before);
}
