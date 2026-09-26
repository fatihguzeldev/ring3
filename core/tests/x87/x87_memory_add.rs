use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const SOURCE: u32 = TOP + 0x10;
const RESULT: u32 = TOP + 0x20;

fn load(opcode: u8, top: f64, source: f64) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([opcode, 0x05]);
    code.extend(SOURCE.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(RESULT.to_le_bytes());
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    if opcode == 0xd8 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "authored m32 inputs are exact"
        )]
        let source = source as f32;
        image
            .memory
            .write(u64::from(SOURCE), &source.to_le_bytes())
            .unwrap();
    } else {
        image
            .memory
            .write(u64::from(SOURCE), &source.to_le_bytes())
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    (cpu, image.memory)
}

fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn frame_add(top: f64, source: f32) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd8, 0x45, 0x08, 0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(RESULT.to_le_bytes());
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
    cpu.set_register(Register32::Ebp, SOURCE - 8);
    cpu.set_x87_control_word(0x0c7f);
    (cpu, image.memory)
}

#[test]
fn memory_add_widths_and_rounding_status() {
    for (opcode, top, source, expected, status) in [
        (0xd8, 2.5, 1.25, 3.75_f64, 0),
        (0xdc, 2.5, 1.25, 3.75, 0),
        (0xd8, 1.0, f64::from(f32::MIN_POSITIVE), 1.0, 0x20),
        (0xdc, 1.0, f64::MIN_POSITIVE, 1.0, 0x20),
        (0xd8, 1.0, -f64::from(f32::MIN_POSITIVE), 1.0, 0x220),
        (0xdc, 1.0, -f64::MIN_POSITIVE, 1.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load(opcode, top, source);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn overflow_stops_without_publishing_result_or_cpu_state() {
    let (mut cpu, mut memory) = load(0xdc, f64::MAX, f64::MAX);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    let value = result(&memory);
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    assert_eq!(result(&memory), value);
}

#[test]
fn masked_infinity_adds_finite_single_memory_and_keeps_zero_divide_status() {
    let zero = TOP + 8;
    let scratch = TOP + 0x18;
    let mut code = vec![0xdd, 0x05]; // FLD m64fp
    code.extend(TOP.to_le_bytes());
    code.extend([0xd8, 0x35]); // FDIV m32fp
    code.extend(zero.to_le_bytes());
    code.extend([0xd9, 0x1d]); // FSTP m32fp
    code.extend(scratch.to_le_bytes());
    code.extend([0xd9, 0x05]); // FLD m32fp
    code.extend(scratch.to_le_bytes());
    code.extend([0xd8, 0x05]); // FADD m32fp
    code.extend(SOURCE.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xdd, 0x1d]); // FNSTSW AX; FSTP m64fp
    code.extend(RESULT.to_le_bytes());
    for (top, source, expected) in [
        (3.0_f64, 0.0_f32, f64::INFINITY.to_bits()),
        (3.0, -0.0, f64::INFINITY.to_bits()),
        (3.0, 1.0, f64::INFINITY.to_bits()),
        (-3.0, 0.0, f64::NEG_INFINITY.to_bits()),
        (-3.0, -1.0, f64::NEG_INFINITY.to_bits()),
    ] {
        for control in [0x007f, 0x027f] {
            let image = load_pe32(&executable::pe32(&code), 16).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_x87_control_word(control);
            let mut memory = image.memory;
            memory.write(u64::from(TOP), &top.to_le_bytes()).unwrap();
            memory.write(u64::from(zero), &0_u32.to_le_bytes()).unwrap();
            memory
                .write(u64::from(SOURCE), &source.to_le_bytes())
                .unwrap();
            assert_eq!(cpu.run(&mut memory, 6).instructions, 6);
            assert_eq!(cpu.register(Register32::Eax) & 0x3a3f, 0x3804);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            assert_eq!(result(&memory), expected);
        }
    }

    let image = load_pe32(&executable::pe32(&code), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    let mut memory = image.memory;
    memory.write(u64::from(TOP), &3_f64.to_le_bytes()).unwrap();
    memory.write(u64::from(zero), &0_u32.to_le_bytes()).unwrap();
    memory
        .write(u64::from(SOURCE), &f32::INFINITY.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn frame_relative_add_truncates_single_precision_sum() {
    for (top, source, expected, status) in [
        (0.5_f64, 0.5_f32, 1.0_f64, 0),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            2.0_f32.powi(-25),
            1.0 + 2.0_f64.powi(-23),
            0x20,
        ),
        (
            -1.0 - 3.0 * 2.0_f64.powi(-24),
            -2.0_f32.powi(-25),
            -1.0 - 2.0_f64.powi(-23),
            0x20,
        ),
        (-0.0, -0.0, -0.0, 0),
    ] {
        let (mut cpu, mut memory) = frame_add(top, source);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn truncating_memory_add_rejects_faults_without_partial_result() {
    for (top, source, control, bad_address, expected) in [
        (1.0_f64, 2.0_f32, 0x0c7f_u16, true, true),
        (f64::MAX, 2.0, 0x0c7f, false, false),
        (1.0, 2.0, 0x087f, false, false),
    ] {
        let (mut cpu, mut memory) = frame_add(top, source);
        cpu.set_x87_control_word(control);
        if bad_address {
            cpu.set_register(Register32::Ebp, 0x5000_0000 - 8);
        }
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        let stop = cpu.run(&mut memory, 1).reason;
        if expected {
            assert!(matches!(stop, StopReason::MemoryFault(_)));
        } else {
            assert_eq!(stop, StopReason::UnsupportedInstruction);
        }
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
    }
}
