use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const INDEXED: u32 = 0x0040_2200;
const TOP: u32 = INDEXED + 8;
const RESULT: u32 = INDEXED + 0x20;

fn load(top: f64, indexed: f64, mode: u8) -> (Cpu32, GuestMemory) {
    let code = [
        0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, 0xde, mode, 0xdf,
        0xe0, 0xdd, 0x1d, 0x20, 0x22, 0x40, 0x00,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(INDEXED), &indexed.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    (cpu, image.memory)
}

fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn load_single(top: f64, indexed: f64) -> (Cpu32, GuestMemory) {
    let code = [
        0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, 0xde, 0xf9, 0xdf,
        0xe0, 0xd9, 0x1d, 0x20, 0x22, 0x40, 0x00,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(INDEXED), &indexed.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    (cpu, image.memory)
}

fn single_result(memory: &GuestMemory) -> f32 {
    let mut bytes = [0; 4];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    f32::from_le_bytes(bytes)
}

#[test]
fn single_precision_divide_pop_rounds_and_updates_status() {
    for (top, indexed, expected, status) in [
        (2.0, 8.0, 4.0_f32, 0),
        (3.0, 1.0, 1.0_f32 / 3.0, 0x220),
        (31.0, 1.0, 1.0_f32 / 31.0, 0x20),
        (3.0, -1.0, -1.0_f32 / 3.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load_single(top, indexed);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(single_result(&memory).to_bits(), expected.to_bits());
    }
}

#[test]
fn single_precision_divide_pop_rejects_zero_and_overflow_atomically() {
    for (top, indexed) in [
        (0.0, 1.0),
        (1.0, f64::from(f32::MAX) * 2.0),
        (2.0, f64::from(f32::MIN_POSITIVE)),
    ] {
        let (mut cpu, mut memory) = load_single(top, indexed);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }

    let (mut cpu, mut memory) = load_single(2.0, 8.0);
    cpu.set_x87_control_word(0x0c7f);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn divide_directions_store_into_second_register_then_pop_top() {
    for (mode, top, indexed, expected, rounding) in [
        (0xf1, 8.0, 2.0, 4.0_f64, 0),
        (0xf1, -8.0, 2.0, -4.0, 0),
        (0xf1, 1.0, 3.0, 1.0 / 3.0, 0x20),
        (0xf1, 10.0, 3.0, 10.0 / 3.0, 0x220),
        (0xf9, 2.0, 8.0, 4.0, 0),
        (0xf9, 3.0, 10.0, 10.0 / 3.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load(top, indexed, mode);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | rounding);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn zero_divisor_and_missing_target_do_not_mutate_stack() {
    for (mode, top, indexed) in [(0xf1, 1.0, 0.0), (0xf9, 0.0, 1.0)] {
        let (mut cpu, mut memory) = load(top, indexed, mode);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
    }

    let code = [0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, 0xde, 0xf1];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &1_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
