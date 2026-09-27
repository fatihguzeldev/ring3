use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const LOWER: u32 = 0x0040_2200;
const Y: u32 = LOWER + 8;
const X: u32 = Y + 8;
const OUTPUT: u32 = X + 8;
const LOWER_OUTPUT: u32 = OUTPUT + 8;

fn fld(address: u32) -> [u8; 6] {
    let [a, b, c, d] = address.to_le_bytes();
    [0xdd, 0x05, a, b, c, d]
}

fn fstp(address: u32) -> [u8; 6] {
    let [a, b, c, d] = address.to_le_bytes();
    [0xdd, 0x1d, a, b, c, d]
}

fn load(y: f64, x: f64, control: u16) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    code.extend(fld(LOWER));
    code.extend(fld(Y));
    code.extend(fld(X));
    code.extend([0xd9, 0xf3, 0xdf, 0xe0]);
    code.extend(fstp(OUTPUT));
    code.extend(fstp(LOWER_OUTPUT));
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    for (address, value) in [(LOWER, 0.75), (Y, y), (X, x)] {
        image
            .memory
            .write(u64::from(address), &f64::to_le_bytes(value))
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn fpatan_uses_y_over_x_and_pops_only_x() {
    for (y, x, expected, precision) in [
        (
            f64::from_bits(0xbf9f_dd5a_0000_0000),
            1.0,
            0xbf9f_dab8_59a6_a59b,
            0x20,
        ),
        (0.0, 1.0, 0, 0),
        (-0.0, 1.0, 0x8000_0000_0000_0000, 0),
        (1.0, 1.0, 0x3fe9_21fb_5444_2d18, 0x20),
        (1.0, -1.0, 0x4002_d97c_7f33_21d2, 0x20),
    ] {
        for control in [0x007f, 0x027f] {
            let (mut cpu, mut memory) = load(y, x, control);
            assert_eq!(cpu.run(&mut memory, 7).instructions, 7);
            assert_eq!(
                cpu.register(Register32::Eax) & 0x3c20,
                0x3000 | precision,
                "{y:?}/{x:?}/{control:x}"
            );
            assert_eq!(read(&memory, OUTPUT), expected);
            assert_eq!(read(&memory, LOWER_OUTPUT), 0.75_f64.to_bits());
        }
    }
}

#[test]
fn fpatan_rejects_missing_operands_and_unmasked_control_atomically() {
    let (mut cpu, mut memory) = load(1.0, 1.0, 0x027f);
    cpu.eip += 18;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    let (mut cpu, mut memory) = load(1.0, 1.0, 0x027f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    cpu.eip += 12;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    for (y, x, control) in [(1.0, 1.0, 0x027e), (17.0, 1.0, 0x027f)] {
        let (mut cpu, mut memory) = load(y, x, 0x027f);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        cpu.set_x87_control_word(control);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}
