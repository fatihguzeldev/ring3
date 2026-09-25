#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const SOURCE: u32 = TOP + 0x10;
const RESULT: u32 = TOP + 0x20;

fn load(top: f64, source: i32) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xda, 0x44, 0x81, 0x50, 0xdf, 0xe0, 0xdd, 0x1d]);
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
    cpu.set_x87_control_word(0x027f);
    cpu.set_register(Register32::Ecx, SOURCE - 0x50 - 12);
    cpu.set_register(Register32::Eax, 3);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn signed_integer_add_preserves_stack_depth_flags_and_budget_behavior() {
    for (top, source, expected) in [
        (1.25_f64, 480_i32, 481.25_f64),
        (-2.0, -3, -5.0),
        (-3.0, 3, 0.0),
        (-0.0, 0, 0.0),
        (0.5, i32::MIN, -2_147_483_647.5),
        (1.0, i32::MAX, 2_147_483_648.0),
    ] {
        for budget in [1, 4] {
            let (mut cpu, mut memory) = load(top, source);
            for _ in 0..4 / budget {
                let run = cpu.run(&mut memory, budget);
                assert_eq!(run.reason, StopReason::InstructionLimit);
                assert_eq!(run.instructions, budget);
            }
            assert_eq!(result(&memory), expected.to_bits());
            assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800);
            assert_eq!(cpu.eflags, 0xced7);
        }
    }
}

#[test]
fn integer_conversion_precedes_single_precision_rounding() {
    for (top, source, expected, status) in [
        (1.0_f64, 16_777_217_i32, 16_777_218.0_f64, 0),
        (0.0, 16_777_217, 16_777_216.0, 0x20),
        (0.0, 16_777_219, 16_777_220.0, 0x220),
        (0.0, -16_777_217, -16_777_216.0, 0x20),
        (0.0, -16_777_219, -16_777_220.0, 0x220),
        (0.0, i32::MAX, 2_147_483_648.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load(top, source);
        cpu.set_x87_control_word(0x007f);
        let run = cpu.run(&mut memory, 3);
        assert_eq!(run.reason, StopReason::InstructionLimit);
        assert_eq!(run.instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn double_precision_ties_report_rounding_direction() {
    for (source, expected, status) in [
        (1, 9_007_199_254_740_992.0_f64, 0x20),
        (3, 9_007_199_254_740_996.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load(9_007_199_254_740_992.0, source);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        assert_eq!(result(&memory), expected.to_bits());
    }
}

#[test]
fn word_integer_add_remains_unsupported() {
    let code = [0xd9, 0xee, 0xde, 0x05, 0x10, 0x22, 0x40, 0x00];
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn invalid_memory_control_stack_and_results_do_not_change_cpu() {
    for case in 0..9 {
        let top = if case == 5 {
            f64::MAX
        } else if case == 6 {
            f64::NAN
        } else {
            1.25
        };
        let (mut cpu, mut memory) = load(top, 480);
        if case == 4 {
            cpu.eip += 6;
        } else {
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        }
        match case {
            0 => cpu.set_register(Register32::Ecx, 0x5000_0000 - 0x50 - 12),
            1 => cpu.set_register(Register32::Ecx, u32::MAX - 0x50 - 12),
            2 => cpu.set_x87_control_word(0x037f),
            3 => cpu.set_x87_control_word(0x027e),
            5 => cpu.set_x87_control_word(0x007f),
            7 => cpu.set_x87_control_word(0x0c7f),
            8 => cpu.set_x87_control_word(0x087f),
            _ => {}
        }
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        assert_eq!(run.instructions, 0);
        if case < 2 {
            assert!(matches!(run.reason, StopReason::MemoryFault(_)));
        } else {
            assert_eq!(run.reason, StopReason::UnsupportedInstruction);
        }
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
    }
}
