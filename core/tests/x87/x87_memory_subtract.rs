use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const SOURCE: u32 = TOP + 0x10;
const RESULT: u32 = TOP + 0x20;

fn load(opcode: u8, mode: u8, top: f64, source: f64) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([opcode, mode]);
    code.extend(SOURCE.to_le_bytes());
    code.extend([0xdf, 0xe0]);
    code.extend([0xdd, 0x1d]);
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

#[test]
fn memory_subtract_directions_widths_and_rounding_status() {
    for (opcode, mode, top, source, expected, status) in [
        (0xd8, 0x25, 2.5, 1.25, 1.25_f64, 0),
        (0xdc, 0x25, 2.5, 1.25, 1.25, 0),
        (0xd8, 0x2d, 2.5, 1.25, -1.25, 0),
        (0xdc, 0x2d, 2.5, 1.25, -1.25, 0),
        (0xd8, 0x25, 1.0, f64::from(f32::MIN_POSITIVE), 1.0, 0x220),
        (0xdc, 0x25, 1.0, f64::MIN_POSITIVE, 1.0, 0x220),
        (0xd8, 0x25, 1.0, -f64::from(f32::MIN_POSITIVE), 1.0, 0x20),
        (0xdc, 0x25, 1.0, -f64::MIN_POSITIVE, 1.0, 0x20),
    ] {
        let (mut cpu, mut memory) = load(opcode, mode, top, source);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(
            cpu.register(Register32::Eax) & 0x220,
            status,
            "{opcode:x} {mode:x} {top} {source}"
        );
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn overflow_stops_before_result_or_status_mutation() {
    let (mut cpu, mut memory) = load(0xdc, 0x25, f64::MAX, -f64::MAX);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before_cpu = cpu;
    let before_result = result(&memory);
    let run = cpu.run(&mut memory, 1);
    assert_eq!(run.reason, StopReason::UnsupportedInstruction);
    assert_eq!(run.instructions, 0);
    assert_eq!(cpu, before_cpu);
    assert_eq!(result(&memory), before_result);
}
