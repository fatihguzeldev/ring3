use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2220;

fn load(angle: f64, control: u16) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(INPUT.to_le_bytes());
    code.extend([0xd9, 0xfe, 0xdf, 0xe0, 0xd9, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &angle.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

#[test]
fn bounded_sine_preserves_signed_zero_and_updates_status() {
    for (angle, expected, precision) in [
        (0.0_f64, 0.0_f32, 0),
        (-0.0, -0.0_f32, 0),
        (0.5, 0.5_f32.sin(), 0x20),
        (-0.5, (-0.5_f32).sin(), 0x20),
    ] {
        let (mut cpu, mut memory) = load(angle, 0x007f);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x3e20, 0x3800 | precision);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut bytes = [0; 4];
        memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), expected.to_bits());
    }
}

#[test]
fn sine_rejects_empty_out_of_range_and_unmasked_controls_atomically() {
    let mut image = load_pe32(&executable::pe32(&[0xd9, 0xfe]), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    for (angle, control) in [(3.0 * std::f64::consts::PI / 4.0, 0x007f), (0.5, 0x0c7f)] {
        let (mut cpu, mut memory) = load(angle, control);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let (mut cpu, mut memory) = load(0.5, 0x007f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    cpu.set_x87_control_word(0x007e);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
