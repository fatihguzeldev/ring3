use super::executable;

use ring3_core::execution::{Cpu32, Register32, load_pe32};

const SOURCE: u32 = 0x0040_2200;
const TOP: u32 = SOURCE + 8;
const RESULT: u32 = SOURCE + 0x20;

#[test]
fn masked_memory_divide_by_zero_stores_signed_infinity() {
    let code = [
        0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, // FLD m64fp
        0xd8, 0x35, 0x00, 0x22, 0x40, 0x00, // FDIV m32fp
        0xdf, 0xe0, // FNSTSW AX
        0xd9, 0x1d, 0x20, 0x22, 0x40, 0x00, // FSTP m32fp
        0xdf, 0xe0, // FNSTSW AX
    ];
    for (top, divisor_bits, expected) in [
        (3.0_f64, 0_u32, f32::INFINITY.to_bits()),
        (-3.0, 0, f32::NEG_INFINITY.to_bits()),
        (3.0, (-0.0_f32).to_bits(), f32::NEG_INFINITY.to_bits()),
    ] {
        for control in [0x007f, 0x027f] {
            let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
            image
                .memory
                .write(u64::from(SOURCE), &divisor_bits.to_le_bytes())
                .unwrap();
            image
                .memory
                .write(u64::from(TOP), &top.to_le_bytes())
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_x87_control_word(control);
            assert_eq!(cpu.run(&mut image.memory, 3).instructions, 3);
            assert_eq!(cpu.register(Register32::Eax) & 0x3a3f, 0x3804);
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            let mut output = [0; 4];
            image.memory.read(u64::from(RESULT), &mut output).unwrap();
            assert_eq!(u32::from_le_bytes(output), expected);
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            assert_eq!(cpu.register(Register32::Eax) & 0x3f, 0x04);
        }
    }
}

#[test]
fn masked_float_zero_over_zero_sets_invalid_and_stores_indefinite() {
    let code = [
        0xdd, 0x05, 0x08, 0x22, 0x40, 0x00, // FLD m64fp
        0xd8, 0x35, 0x00, 0x22, 0x40, 0x00, // FDIV m32fp
        0xdf, 0xe0, // FNSTSW AX
        0xdd, 0x1d, 0x20, 0x22, 0x40, 0x00, // FSTP m64fp
    ];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &0.0_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut image.memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax) & 0x3f, 1);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let mut output = [0; 8];
    image.memory.read(u64::from(RESULT), &mut output).unwrap();
    assert_eq!(u64::from_le_bytes(output), 0xfff8_0000_0000_0000);
}

#[test]
fn masked_integer_zero_over_zero_produces_indefinite_and_propagates_through_multiply() {
    let code = [
        0xdb, 0x05, 0x08, 0x22, 0x40, 0x00, // FILD m32int (zero)
        0xda, 0x35, 0x00, 0x22, 0x40, 0x00, // FIDIV m32int (zero)
        0xdf, 0xe0, // FNSTSW AX
        0xdb, 0x05, 0x0c, 0x22, 0x40, 0x00, // FILD m32int (two)
        0xd8, 0xc9, // FMUL ST0, ST1
        0xdd, 0x1d, 0x20, 0x22, 0x40, 0x00, // FSTP m64fp
    ];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(SOURCE + 12), &2_i32.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut image.memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax) & 0x3f, 1);
    assert_eq!(cpu.run(&mut image.memory, 3).instructions, 3);
    let mut output = [0; 8];
    image.memory.read(u64::from(RESULT), &mut output).unwrap();
    assert_eq!(u64::from_le_bytes(output), 0xfff8_0000_0000_0000);
}
