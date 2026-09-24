#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const SOURCE: u32 = 0x0040_2200;
const TOP: u32 = SOURCE + 8;
const RESULT: u32 = SOURCE + 0x20;
const REMAINDER: u32 = RESULT + 8;

fn load(numerator: f64, denominator: f64, control: u16) -> (Cpu32, GuestMemory) {
    let code = [
        0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, 0xd8, 0xf1, 0xdf,
        0xe0, 0xd9, 0x1d, 0x20, 0x22, 0x40, 0x00, 0xdd, 0x1d, 0x28, 0x22, 0x40, 0x00,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(SOURCE), &denominator.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(u64::from(TOP), &numerator.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

fn read32(memory: &GuestMemory, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn single_precision_register_divide_preserves_source_and_rounds_quotient() {
    for (numerator, denominator, expected, status) in [
        (8.0, 2.0, 4.0_f32, 0),
        (1.0, 3.0, 1.0_f32 / 3.0, 0x220),
        (1.0, 31.0, 1.0_f32 / 31.0, 0x20),
        (-1.0, 3.0, -1.0_f32 / 3.0, 0x20),
    ] {
        let (mut cpu, mut memory) = load(numerator, denominator, 0x007f);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3000 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(read32(&memory, RESULT), expected.to_bits());
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut bytes = [0; 8];
        memory.read(u64::from(REMAINDER), &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), denominator.to_bits());
    }
}

#[test]
fn register_divide_rejects_bad_source_range_and_control_atomically() {
    for (numerator, denominator, control) in [
        (1.0, 0.0, 0x007f),
        (f64::from(f32::MAX) * 2.0, 1.0, 0x007f),
        (f64::from(f32::MIN_POSITIVE), 2.0, 0x007f),
        (8.0, 2.0, 0x0c7f),
    ] {
        let (mut cpu, mut memory) = load(numerator, denominator, control);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let code = [0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, 0xd8, 0xf1];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &8_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
