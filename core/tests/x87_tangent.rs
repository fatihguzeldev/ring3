#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const ONE: u32 = 0x0040_2220;
const TANGENT: u32 = ONE + 8;

fn load(code: &[u8], angle: f64, control: u16) -> (Cpu32, GuestMemory) {
    let mut image = load_pe32(&executable::pe32(code), 16).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &angle.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

fn fld() -> Vec<u8> {
    let mut code = vec![0xdd, 0x05];
    code.extend(INPUT.to_le_bytes());
    code
}

fn fstp(address: u32) -> Vec<u8> {
    let mut code = vec![0xd9, 0x1d];
    code.extend(address.to_le_bytes());
    code
}

fn read(memory: &GuestMemory, address: u32) -> f64 {
    let mut bytes = [0; 4];
    memory.read(u64::from(address), &mut bytes).unwrap();
    f64::from(f32::from_le_bytes(bytes))
}

#[test]
fn bounded_tangent_pushes_one_above_tangent_and_sets_status() {
    let mut code = fld();
    code.extend([0xd9, 0xf2, 0xdf, 0xe0]);
    code.extend(fstp(ONE));
    code.extend(fstp(TANGENT));
    for (angle, expected, precision) in [
        (0.0_f64, 0.0_f64, 0),
        (-0.0, -0.0, 0),
        (0.5, 0.546_302_489_843_790_5, 0x20),
        (-0.5, -0.546_302_489_843_790_5, 0x20),
    ] {
        let (mut cpu, mut memory) = load(&code, angle, 0x007f);
        assert_eq!(cpu.run(&mut memory, 5).instructions, 5);
        assert_eq!(cpu.register(Register32::Eax) & 0x3c20, 0x3000 | precision);
        assert_eq!(read(&memory, ONE).to_bits(), 1.0_f64.to_bits());
        let actual = read(&memory, TANGENT);
        if angle == 0.0 {
            assert_eq!(actual.to_bits(), expected.to_bits());
        } else {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn tangent_rejects_empty_full_out_of_range_and_unsupported_control_atomically() {
    let (mut cpu, mut memory) = load(&[0xd9, 0xf2], 0.5, 0x007f);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    let mut code = fld();
    code.extend([0xd9, 0xf2]);
    for (angle, control) in [(1.25, 0x007f), (0.5, 0x0c7f), (f64::NAN, 0x007f)] {
        let (mut cpu, mut memory) = load(&code, angle, control);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }

    let (mut cpu, mut memory) = load(&code, 0.5, 0x007f);
    let entry = cpu.eip;
    for _ in 0..8 {
        cpu.eip = entry;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    }
    cpu.eip = entry + 6;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
