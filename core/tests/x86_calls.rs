#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{
    Cpu32, LoadedPe32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32,
};

fn program(code: &[u8]) -> (LoadedPe32, Cpu32) {
    let mut image = load_pe32(&executable::pe32(code), 4).unwrap();
    image
        .memory
        .map_zeroed(0x1000_0000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Esp, 0x1000_1000);
    (image, cpu)
}

#[test]
fn function_uses_stack_arguments_frame_and_return_address() {
    let code = [
        0x6a, 35, 0x6a, 7, 0xe8, 9, 0, 0, 0, 0x83, 0xc4, 8, 0xa3, 0, 0x20, 0x40, 0, 0xcc, 0x55,
        0x89, 0xe5, 0x8b, 0x45, 8, 0x03, 0x45, 12, 0xc9, 0xc3,
    ];
    let (mut image, mut cpu) = program(&code);
    let result = cpu.run(&mut image.memory, 100);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 12);
    assert_eq!(cpu.register(Register32::Eax), 42);
    assert_eq!(cpu.register(Register32::Esp), 0x1000_1000);
    assert_eq!(cpu.register(Register32::Ebp), 0);
    let mut bytes = [0; 4];
    image.memory.read(0x0040_2000, &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 42);
    image.memory.read(0x1000_0ff4, &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), image.entry_point + 9);
}

#[test]
fn pushing_and_popping_esp_observe_x86_ordering() {
    let (mut image, mut cpu) = program(&[0x54, 0x58, 0x68, 0xf0, 0x0f, 0, 0x10, 0x5c, 0xcc]);
    assert_eq!(
        cpu.run(&mut image.memory, 10).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 0x1000_1000);
    assert_eq!(cpu.register(Register32::Esp), 0x1000_0ff0);
}

#[test]
fn indirect_call_reads_target_before_pushing_its_return_address() {
    let (mut image, mut cpu) = program(&[0xff, 0x14, 0x24, 0xcc, 0xb8, 42, 0, 0, 0, 0xc3]);
    cpu.set_register(Register32::Esp, 0x1000_0ffc);
    image
        .memory
        .write(0x1000_0ffc, &(image.entry_point + 4).to_le_bytes())
        .unwrap();
    assert_eq!(
        cpu.run(&mut image.memory, 10).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 42);
    assert_eq!(cpu.register(Register32::Esp), 0x1000_0ffc);
}

#[test]
fn ret_immediate_removes_arguments_after_reading_return_address() {
    let (mut image, mut cpu) = program(&[0xc2, 8, 0, 0xcc]);
    cpu.set_register(Register32::Esp, 0x1000_0ff0);
    image
        .memory
        .write(0x1000_0ff0, &(image.entry_point + 3).to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut image.memory, 5).reason, StopReason::Breakpoint);
    assert_eq!(cpu.register(Register32::Esp), 0x1000_0ffc);
}

#[test]
fn stack_faults_preserve_the_faulting_cpu_state() {
    for code in [&[0x50][..], &[0xe8, 0, 0, 0, 0], &[0x58], &[0xc3], &[0xc9]] {
        let (mut image, mut cpu) = program(code);
        cpu.set_register(Register32::Esp, 0x2000_0000);
        cpu.set_register(Register32::Ebp, 0x2000_0000);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(
            matches!(result.reason, StopReason::MemoryFault(_)),
            "{code:?}: {result:?}"
        );
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}
