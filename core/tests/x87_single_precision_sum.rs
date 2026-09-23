#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const SOURCE: u32 = TOP + 0x10;
const OUTPUT: u32 = TOP + 0x20;

fn load(operation: u8, top: f64, source: f32, source_address: u32) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd8, operation]);
    code.extend(source_address.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xd9, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(u64::from(SOURCE), &source.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn output(memory: &GuestMemory) -> u32 {
    let mut bytes = [0; 4];
    memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn memory_add_subtract_and_reverse_subtract_keep_direction_and_signed_zero() {
    for (operation, top, source, expected) in [
        (0x05, 2.0, 5.0, 7.0_f32),
        (0x25, 2.0, 5.0, -3.0),
        (0x2d, 2.0, 5.0, 3.0),
        (0x25, 1.0, 1.0, 0.0),
        (0x2d, 0.0, -0.0, -0.0),
    ] {
        let (mut cpu, mut memory) = load(operation, top, source, SOURCE);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(output(&memory), expected.to_bits());
        assert_eq!(cpu.register(Register32::Eax) & 0x220, 0);
        assert_eq!(cpu.eflags, 0xced7);
    }
}

#[test]
fn final_single_precision_rounding_controls_precision_and_c1() {
    for (top, source, expected, status) in [
        (1.0, 2.0_f32.powi(-24), 1.0_f32, 0x20),
        (
            1.0 + 2.0_f64.powi(-23),
            2.0_f32.powi(-24),
            1.0_f32 + 2.0_f32.powi(-22),
            0x220,
        ),
        (1.0, f32::MIN_POSITIVE, 1.0_f32, 0x20),
        (-1.0, -2.0_f32.powi(-24), -1.0_f32, 0x220),
        (
            -1.0 - 2.0_f64.powi(-23),
            -2.0_f32.powi(-24),
            -1.0_f32 - 2.0_f32.powi(-22),
            0x20,
        ),
    ] {
        let (mut cpu, mut memory) = load(0x05, top, source, SOURCE);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(output(&memory), expected.to_bits());
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
    }
}

#[test]
fn memory_fault_overflow_control_and_empty_stack_are_atomic() {
    for case in 0..4 {
        let address = if case == 0 { 0x5000_0000 } else { SOURCE };
        let top = if case == 1 { f64::MAX } else { 2.0 };
        let (mut cpu, mut memory) = load(0x2d, top, 5.0, address);
        if case == 3 {
            cpu.eip += 6;
        } else {
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        }
        if case == 2 {
            cpu.set_x87_control_word(0x037f);
        }
        let before = cpu;
        let stop = cpu.run(&mut memory, 1).reason;
        if case == 0 {
            assert!(matches!(stop, StopReason::MemoryFault(_)));
        } else {
            assert_eq!(stop, StopReason::UnsupportedInstruction);
        }
        assert_eq!(cpu, before);
        assert_eq!(output(&memory), 0);
    }
}
