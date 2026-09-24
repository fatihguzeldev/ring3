#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const SOURCE: u32 = 0x0040_2200;
const TOP: u32 = SOURCE + 8;
const RESULT: u32 = SOURCE + 0x20;

fn load(top: f64, source: Option<f64>, opcode: u8) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    if source.is_some() {
        code.extend([0xdd, 0x05]);
        code.extend(SOURCE.to_le_bytes());
    }
    code.extend([0xdd, 0x05]);
    code.extend(TOP.to_le_bytes());
    code.extend([opcode, if source.is_some() { 0xc1 } else { 0xc0 }]);
    code.extend([0xdf, 0xe0]);
    if opcode == 0xdc && source.is_some() {
        code.extend([0xdd, 0x1d]);
        code.extend((RESULT + 8).to_le_bytes());
    }
    code.extend([0xdd, 0x1d]);
    code.extend(RESULT.to_le_bytes());
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    if let Some(source) = source {
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

fn load_subtract_pop(indexed: f64, top: f64, control: u16) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for address in [SOURCE, TOP] {
        code.extend([0xdd, 0x05]);
        code.extend(address.to_le_bytes());
    }
    code.extend([0xde, 0xe9, 0xdf, 0xe0, 0xd9, 0x1d]);
    code.extend(RESULT.to_le_bytes());
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    for (address, value) in [(SOURCE, indexed), (TOP, top)] {
        image
            .memory
            .write(u64::from(address), &value.to_le_bytes())
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

fn load_reverse_subtract_pop(
    indexed: f64,
    middle: f64,
    top: f64,
    control: u16,
) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for address in [SOURCE, SOURCE + 0x10, TOP] {
        code.extend([0xdd, 0x05]);
        code.extend(address.to_le_bytes());
    }
    code.extend([0xde, 0xe2, 0xdf, 0xe0]);
    for address in [RESULT, RESULT + 4] {
        code.extend([0xd9, 0x1d]);
        code.extend(address.to_le_bytes());
    }
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    for (address, value) in [(SOURCE, indexed), (SOURCE + 0x10, middle), (TOP, top)] {
        image
            .memory
            .write(u64::from(address), &value.to_le_bytes())
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

#[test]
fn reverse_subtract_pop_writes_indexed_result_and_keeps_middle_register() {
    let quarter_ulp = f64::from(f32::EPSILON) * 0.25;
    for (indexed, top, expected, status) in [
        (3.0, 5.0, 2.0_f32, 0),
        (3.0, -1.0, -4.0_f32, 0),
        (1.0, 2.0 + quarter_ulp, 1.0_f32, 0x20),
        (1.0, 2.0 - quarter_ulp, 1.0_f32, 0x220),
    ] {
        let (mut cpu, mut memory) = load_reverse_subtract_pop(indexed, 7.0, top, 0x007f);
        assert_eq!(cpu.run(&mut memory, 5).instructions, 5);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3000 | status);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        for (address, value) in [(RESULT, 7.0_f32), (RESULT + 4, expected)] {
            let mut bytes = [0; 4];
            memory.read(u64::from(address), &mut bytes).unwrap();
            assert_eq!(u32::from_le_bytes(bytes), value.to_bits());
        }
    }
}

#[test]
fn reverse_subtract_pop_rejects_missing_target_range_and_control_atomically() {
    for (indexed, top, control) in [
        (-f64::MAX, f64::MAX, 0x007f),
        (
            f64::from(f32::MIN_POSITIVE) / 2.0,
            f64::from(f32::MIN_POSITIVE),
            0x007f,
        ),
        (3.0, 5.0, 0x0c7f),
    ] {
        let (mut cpu, mut memory) = load_reverse_subtract_pop(indexed, 7.0, top, control);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }

    let (mut cpu, mut memory) = load_reverse_subtract_pop(3.0, 7.0, 5.0, 0x007f);
    let entry = cpu.eip;
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    cpu.eip = entry + 18;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn subtract_pop_stores_indexed_minus_top_with_single_precision() {
    let quarter_ulp = f64::from(f32::EPSILON) * 0.25;
    for (indexed, top, expected, status) in [
        (2.0, 0.0, 2.0_f32, 0),
        (3.0, 1.0, 2.0_f32, 0),
        (1.0, 3.0, -2.0_f32, 0),
        (1.0, quarter_ulp, 1.0_f32, 0x220),
        (1.0, -quarter_ulp, 1.0_f32, 0x20),
    ] {
        let (mut cpu, mut memory) = load_subtract_pop(indexed, top, 0x007f);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut bytes = [0; 4];
        memory.read(u64::from(RESULT), &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), expected.to_bits());
    }
}

#[test]
fn subtract_pop_rejects_missing_target_range_and_control_atomically() {
    for (indexed, top, control) in [
        (f64::MAX, -f64::MAX, 0x007f),
        (
            f64::from(f32::MIN_POSITIVE),
            f64::from(f32::MIN_POSITIVE) / 2.0,
            0x007f,
        ),
        (3.0, 1.0, 0x0c7f),
    ] {
        let (mut cpu, mut memory) = load_subtract_pop(indexed, top, control);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }

    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xde, 0xe9]);
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &1.0_f64.to_le_bytes())
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

#[test]
fn self_and_second_register_add_set_result_and_rounding() {
    for (top, source, opcode, expected, status) in [
        (1.5, None, 0xd8, 3.0_f64, 0),
        (1.5, None, 0xdc, 3.0, 0),
        (2.5, Some(1.25), 0xd8, 3.75, 0),
        (2.5, Some(1.25), 0xdc, 3.75, 0),
        (1.0, Some(f64::MIN_POSITIVE), 0xd8, 1.0, 0x20),
        (1.0, Some(-f64::MIN_POSITIVE), 0xd8, 1.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load(top, source, opcode);
        let steps = 3 + u64::from(source.is_some());
        assert_eq!(cpu.run(&mut memory, steps).instructions, steps);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        if opcode == 0xdc && source.is_some() {
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let mut bytes = [0; 8];
            memory.read(u64::from(RESULT + 8), &mut bytes).unwrap();
            assert_eq!(u64::from_le_bytes(bytes), top.to_bits());
        }
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn add_pop_stores_the_rounded_second_register_and_pops_top() {
    for (top, source, control, expected, status) in [
        (0.0_f64, 0.0_f64, 0x007f_u16, 0.0_f64, 0),
        (2.5, 1.25, 0x027f, 3.75, 0),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            2.0_f64.powi(-25),
            0x007f,
            1.0 + 2.0_f64.powi(-22),
            0x220,
        ),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            2.0_f64.powi(-25),
            0x0c7f,
            1.0 + 2.0_f64.powi(-23),
            0x20,
        ),
    ] {
        let (mut cpu, mut memory) = load(top, Some(source), 0xde);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn add_pop_rejects_missing_target_and_overflow_atomically() {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xde, 0xc1]);
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

    let (mut cpu, mut memory) = load(f64::MAX, Some(f64::MAX), 0xde);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn overflow_and_empty_register_do_not_publish_partial_result() {
    let (mut cpu, mut memory) = load(f64::MAX, None, 0xdc);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    assert_eq!(result(&memory), 0);

    let mut image = load_pe32(&executable::pe32(&[0xd8, 0xc0]), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn single_precision_register_add_rounds_nearest_or_toward_zero() {
    for (top, source, control, expected, status) in [
        (0.5_f64, 0.5_f64, 0x0c7f_u16, 1.0_f64, 0),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            2.0_f64.powi(-25),
            0x0c7f,
            1.0 + 2.0_f64.powi(-23),
            0x20,
        ),
        (
            -1.0 - 3.0 * 2.0_f64.powi(-24),
            -2.0_f64.powi(-25),
            0x0c7f,
            -1.0 - 2.0_f64.powi(-23),
            0x20,
        ),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            2.0_f64.powi(-25),
            0x007f,
            1.0 + 2.0_f64.powi(-22),
            0x220,
        ),
        (-0.0, -0.0, 0x0c7f, -0.0, 0),
    ] {
        let (mut cpu, mut memory) = load(top, Some(source), 0xd8);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn single_precision_register_add_rejects_bad_operands_atomically() {
    for (top, source, opcode, control) in [
        (f64::MAX, f64::MAX, 0xd8, 0x0c7f),
        (1.0, 2.0, 0xdc, 0x0c7f),
        (1.0, 2.0, 0xd8, 0x087f),
    ] {
        let (mut cpu, mut memory) = load(top, Some(source), opcode);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
    }

    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd8, 0xc1]);
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &1.0_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x0c7f);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
