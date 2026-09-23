#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const SOURCE: u32 = 0x0040_2200;
const MIDDLE: u32 = SOURCE + 8;
const TOP: u32 = SOURCE + 16;
const RESULT: u32 = SOURCE + 0x20;

fn load(source: f64, middle: f64, top: f64) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for address in [SOURCE, MIDDLE, TOP] {
        code.extend([0xdd, 0x05]);
        code.extend(address.to_le_bytes());
    }
    code.extend([0xd8, 0xe2, 0xdf, 0xe0]);
    for address in [RESULT, RESULT + 8, RESULT + 16] {
        code.extend([0xdd, 0x1d]);
        code.extend(address.to_le_bytes());
    }
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    for (address, value) in [(SOURCE, source), (MIDDLE, middle), (TOP, top)] {
        image
            .memory
            .write(u64::from(address), &value.to_le_bytes())
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    (cpu, image.memory)
}

fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn subtracts_third_register_into_top_without_popping_and_reports_rounding() {
    for (source, middle, top, expected, status) in [
        (2.0, 4.0, 5.0, 3.0_f64, 0),
        (f64::MIN_POSITIVE, 4.0, 1.0, 1.0, 0x220),
        (-f64::MIN_POSITIVE, 4.0, 1.0, 1.0, 0x20),
    ] {
        let (mut cpu, mut memory) = load(source, middle, top);
        assert_eq!(cpu.run(&mut memory, 5).instructions, 5);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x2800 | status);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(read(&memory, RESULT), expected.to_bits());
        assert_eq!(read(&memory, RESULT + 8), middle.to_bits());
        assert_eq!(read(&memory, RESULT + 16), source.to_bits());
    }
}

#[test]
fn missing_third_register_and_overflow_leave_the_stack_unchanged() {
    let (mut cpu, mut memory) = load(2.0, 4.0, 5.0);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    cpu.eip += 6;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    assert_eq!(read(&memory, RESULT), 0);

    let (mut cpu, mut memory) = load(-f64::MAX, 4.0, f64::MAX);
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    assert_eq!(read(&memory, RESULT), 0);
}
