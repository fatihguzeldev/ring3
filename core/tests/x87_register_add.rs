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
