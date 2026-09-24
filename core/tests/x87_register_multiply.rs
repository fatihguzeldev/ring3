#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2300;

fn load(code: &[u8]) -> (Cpu32, ring3_core::execution::GuestMemory) {
    let image = load_pe32(&executable::pe32(code), 32).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn fld(code: &mut Vec<u8>, address: u32) {
    code.extend([0xdd, 0x05]);
    code.extend(address.to_le_bytes());
}

fn fstp(code: &mut Vec<u8>, address: u32) {
    code.extend([0xdd, 0x1d]);
    code.extend(address.to_le_bytes());
}

fn multiply_pop_fixture(
    indexed: f64,
    top: f64,
    control: u16,
) -> (Cpu32, ring3_core::execution::GuestMemory) {
    let mut code = Vec::new();
    fld(&mut code, INPUT);
    fld(&mut code, INPUT + 8);
    code.extend([0xde, 0xc9, 0xdf, 0xe0, 0xd9, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    let (mut cpu, mut memory) = load(&code);
    cpu.set_x87_control_word(control);
    memory
        .write(u64::from(INPUT), &indexed.to_le_bytes())
        .unwrap();
    memory
        .write(u64::from(INPUT + 8), &top.to_le_bytes())
        .unwrap();
    (cpu, memory)
}

#[test]
fn register_multiply_pop_rounds_and_removes_top() {
    let quarter_ulp = f64::from(f32::EPSILON) * 0.25;
    for (indexed, top, expected, status) in [
        (2.0, 3.0, 6.0_f32, 0),
        (1.0 + quarter_ulp, 1.0, 1.0_f32, 0x20),
        (-1.0 - quarter_ulp, 1.0, -1.0_f32, 0x220),
        (-0.0, 2.0, -0.0_f32, 0),
    ] {
        let (mut cpu, mut memory) = multiply_pop_fixture(indexed, top, 0x007f);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut bytes = [0; 4];
        memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), expected.to_bits());
    }
}

#[test]
fn register_multiply_pop_rejects_range_and_control_atomically() {
    for (indexed, top, control) in [
        (f64::from(f32::MAX) * 2.0, 1.0, 0x007f),
        (f64::from(f32::MIN_POSITIVE), 0.5, 0x007f),
        (2.0, 3.0, 0x0c7f),
    ] {
        let (mut cpu, mut memory) = multiply_pop_fixture(indexed, top, control);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn register_multiply_uses_each_occupied_source_without_popping() {
    for index in 0_u8..8 {
        let mut code = Vec::new();
        for slot in 0..8_u32 {
            fld(&mut code, INPUT + slot * 8);
        }
        code.extend([0xd8, 0xc8 + index]);
        for slot in 0..8_u32 {
            fstp(&mut code, OUTPUT + slot * 8);
        }
        let (mut cpu, mut memory) = load(&code);
        for slot in 0..8_u32 {
            memory
                .write(
                    u64::from(INPUT + slot * 8),
                    &f64::from(slot + 1).to_le_bytes(),
                )
                .unwrap();
        }
        assert_eq!(cpu.run(&mut memory, 9).instructions, 9);
        assert_eq!(cpu.eflags, 0xced7);
        assert_eq!(cpu.x87_control_word(), 0x027f);
        assert_eq!(cpu.run(&mut memory, 8).instructions, 8);
        for slot in 0..8_u32 {
            let mut bytes = [0; 8];
            memory
                .read(u64::from(OUTPUT + slot * 8), &mut bytes)
                .unwrap();
            let expected = if slot == 0 {
                8_f64 * f64::from(8 - u32::from(index))
            } else {
                f64::from(8 - slot)
            };
            assert_eq!(u64::from_le_bytes(bytes), expected.to_bits());
        }
    }
}

#[test]
fn register_multiply_preserves_signed_zero_and_reports_rounding() {
    for (top, source, control, expected, status) in [
        (
            (-0_f64).to_bits(),
            2_f64.to_bits(),
            0x027f,
            (-0_f64).to_bits(),
            0,
        ),
        (
            1_000_000_000_f64.to_bits(),
            1_000_000_000_f64.to_bits(),
            0x007f,
            0x43ab_c16d_6000_0000,
            0x20,
        ),
    ] {
        let mut code = Vec::new();
        fld(&mut code, INPUT);
        fld(&mut code, INPUT + 8);
        code.extend([0xd8, 0xc9, 0xdf, 0xe0]);
        fstp(&mut code, OUTPUT);
        let (mut cpu, mut memory) = load(&code);
        cpu.set_x87_control_word(control);
        memory
            .write(u64::from(INPUT), &source.to_le_bytes())
            .unwrap();
        memory
            .write(u64::from(INPUT + 8), &top.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut bytes = [0; 8];
        memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), expected);
    }
}

#[test]
fn register_multiply_rejects_empty_source_and_unmasked_control_atomically() {
    let mut code = Vec::new();
    fld(&mut code, INPUT);
    code.extend([0xd8, 0xc9, 0xd8, 0xc8]);
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &2_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    cpu.eip += 2;
    cpu.set_x87_control_word(0x027e);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
