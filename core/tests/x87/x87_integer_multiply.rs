use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const SOURCE: u32 = TOP + 0x10;
const RESULT: u32 = TOP + 0x20;

fn load(top: f64, source: i32, source_address: u32) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xda, 0x0d]);
    code.extend(source_address.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(RESULT.to_le_bytes());
    code.push(0xcc);
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

fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn signed_integer_source_is_multiplied_without_interpreting_float_bits() {
    for (top, source, expected) in [
        (1.25_f64, 480_i32, 600.0_f64),
        (-2.0, -3, 6.0),
        (-0.0, 0, -0.0),
        (0.5, i32::MIN, -1_073_741_824.0),
        (1.0, i32::MAX, 2_147_483_647.0),
    ] {
        for budget in [1, 4] {
            let (mut cpu, mut memory) = load(top, source, SOURCE);
            cpu.set_x87_control_word(0x027f);
            let mut steps = 0;
            while steps < 4 {
                let run = cpu.run(&mut memory, budget);
                assert_eq!(run.reason, StopReason::InstructionLimit);
                steps += run.instructions;
            }
            assert_eq!(steps, 4);
            assert_eq!(result(&memory), expected.to_bits());
            assert_eq!(cpu.eflags, 0xced7);
        }
    }
}

#[test]
fn single_precision_rounding_sets_precision_status() {
    let (mut cpu, mut memory) = load(1.0 + 2.0_f64.powi(-24), 1, SOURCE);
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax) & 0x220, 0x20);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(result(&memory), 1.0_f64.to_bits());
}

#[test]
fn invalid_source_control_and_empty_stack_leave_cpu_unchanged() {
    for case in 0..3 {
        let source_address = if case == 0 { 0x5000_0000 } else { SOURCE };
        let (mut cpu, mut memory) = load(1.25, 480, source_address);
        if case == 2 {
            cpu.eip += 6;
        } else {
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        }
        if case == 1 {
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
        assert_eq!(result(&memory), 0);
    }
}
