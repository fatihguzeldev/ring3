#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const SOURCE: u32 = TOP + 0x10;
const OUTPUT: u32 = TOP + 0x20;

fn load(code: &[u8], top: f64, source: f32, control: u16) -> (Cpu32, GuestMemory) {
    let mut image = load_pe32(&executable::pe32(code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(u64::from(SOURCE), &source.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn output(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn absolute_value_clears_only_the_sign_for_both_supported_precisions() {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd9, 0xe1, 0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    for control in [0x007f, 0x027f] {
        for input in [-1.0 - 2.0_f64.powi(-30), -0.0, 0.0, 5.0] {
            let (mut cpu, mut memory) = load(&code, input, 0.0, control);
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            cpu.set_x87_control_word(0x027f);
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            assert_eq!(output(&memory), input.to_bits() & !(1_u64 << 63));
            assert_eq!(cpu.register(Register32::Eax) & 0x220, 0);
            assert_eq!(cpu.eflags, 0xced7);
        }
    }
}

#[test]
fn change_sign_flips_only_sign_bit_for_both_supported_precisions() {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd9, 0xe0, 0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    for control in [0x007f, 0x027f] {
        for input in [-1.0 - 2.0_f64.powi(-30), -0.0, 0.0, 5.0] {
            let (mut cpu, mut memory) = load(&code, input, 0.0, control);
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800);
            cpu.set_x87_control_word(0x027f);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            assert_eq!(output(&memory), input.to_bits() ^ (1_u64 << 63));
            assert_eq!(cpu.eflags, 0xced7);
        }
    }
}

#[test]
fn change_sign_rejects_empty_stack_and_unmasked_control_atomically() {
    let (mut cpu, mut memory) = load(&[0xd9, 0xe0], 2.0, 0.0, 0x007f);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd9, 0xe0]);
    let (mut cpu, mut memory) = load(&code, 2.0, 0.0, 0x007f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    cpu.set_x87_control_word(0x007e);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn absolute_value_clears_c1_and_keeps_sticky_precision() {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd8, 0x05]);
    code.extend(SOURCE.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xd9, 0xe1, 0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    let (mut cpu, mut memory) = load(&code, -1.0, -2.0_f32.powi(-24), 0x007f);
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax) & 0x220, 0x220);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(cpu.register(Register32::Eax) & 0x220, 0x20);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(output(&memory), 1.0_f64.to_bits());
    assert_eq!(cpu.eflags, 0xced7);
}

#[test]
fn empty_stack_and_unsupported_control_leave_cpu_unchanged() {
    for case in 0..3 {
        let (mut cpu, mut memory) = load(&[0xd9, 0xe1], -2.0, 0.0, 0x007f);
        if case != 0 {
            let mut code = vec![0xdd, 0x05];
            code.extend(TOP.to_le_bytes());
            code.extend([0xd9, 0xe1]);
            (cpu, memory) = load(&code, -2.0, 0.0, 0x007f);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            cpu.set_x87_control_word(if case == 1 { 0x007e } else { 0x037f });
        }
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(output(&memory), 0);
    }
}
