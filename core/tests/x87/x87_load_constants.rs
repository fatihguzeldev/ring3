use super::constant_load_cases;

#[test]
fn exact_constants_preserve_lower_stack_and_control_across_budgets() {
    constant_load_cases::exact_constants_preserve_lower_stack_across_budgets();
}

use constant_load_cases::{INPUT, OUTPUT, load, read};
use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason};

fn refuses(cpu: &mut Cpu32, memory: &mut GuestMemory, reason: &StopReason) {
    let before = *cpu;
    let output = read(memory, OUTPUT);
    let run = cpu.run(memory, 1);
    assert_eq!(&run.reason, reason);
    assert_eq!(run.instructions, 0);
    assert_eq!(*cpu, before);
    assert_eq!(read(memory, OUTPUT), output);
}

#[test]
fn constant_push_accesses_no_data_memory_and_preserves_unrelated_cpu_state() {
    for constant in [0xee, 0xe8] {
        let value = if constant == 0xee { 0.0_f64 } else { 1.0 };
        let (mut cpu, mut memory) = load(&[5.0], constant);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let (mut expected, mut other) = load(&[5.0, value], constant);
        assert_eq!(expected.run(&mut other, 2).instructions, 2);
        expected.eip = cpu.eip + 2;
        let pages = memory.mapped_pages();
        memory
            .protect(0x0040_2000, 4096, Permissions::NONE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(cpu, expected);
        assert_eq!(memory.mapped_pages(), pages);
    }
}

#[test]
fn full_stack_refuses_atomically_and_pop_allows_same_instruction_retry() {
    for constant in [0xee, 0xe8] {
        let value = if constant == 0xee { 0.0_f64 } else { 1.0 };
        let values = [1., 2., 3., 4., 5., 6., 7.];
        let (mut cpu, mut memory) = load(&values, constant);
        assert_eq!(cpu.run(&mut memory, 7).instructions, 7);
        let instruction = cpu.eip;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        cpu.eip = instruction;
        refuses(&mut cpu, &mut memory, &StopReason::UnsupportedInstruction);
        cpu.eip = instruction + 4;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(read(&memory, OUTPUT), value.to_bits());
        cpu.eip = instruction;
        assert_eq!(cpu.run(&mut memory, 30).reason, StopReason::Breakpoint);
        assert_eq!(read(&memory, OUTPUT), value.to_bits());
        for (i, value) in values.iter().rev().enumerate() {
            assert_eq!(
                read(&memory, OUTPUT + u32::try_from(i + 1).unwrap() * 8),
                value.to_bits()
            );
        }
        cpu.eip = instruction + 4;
        refuses(&mut cpu, &mut memory, &StopReason::UnsupportedInstruction);
    }
}

#[test]
fn each_unmasked_exception_profile_refuses_and_control_repair_retries() {
    for constant in [0xee, 0xe8] {
        let value = if constant == 0xee { 0.0_f64 } else { 1.0 };
        for bit in 0..6 {
            let (mut cpu, mut memory) = load(&[7.0], constant);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            cpu.set_x87_control_word(0x7f & !(1 << bit));
            refuses(&mut cpu, &mut memory, &StopReason::UnsupportedInstruction);
            cpu.set_x87_control_word(0x007f);
            assert_eq!(cpu.run(&mut memory, 30).reason, StopReason::Breakpoint);
            assert_eq!(read(&memory, OUTPUT), value.to_bits());
            assert_eq!(read(&memory, OUTPUT + 8), 7_f64.to_bits());
        }
    }
}

fn replace_code(memory: &mut GuestMemory, address: u32, code: &[u8]) {
    memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(u64::from(address), code).unwrap();
    memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
}

#[test]
fn constant_load_clears_round_up_and_preserves_sticky_exceptions_and_conditions() {
    for constant in [0xee, 0xe8] {
        for (comparison, condition) in [(10_f64, 0x4000), (20_f64, 0x100)] {
            let values = [
                f64::from_bits(0x7ff0_0000_0000_0001),
                f64::from_bits(1),
                10.0,
            ];
            let (mut cpu, mut memory) = load(&values, constant);
            cpu.set_x87_control_word(0x027f);
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            memory
                .write(u64::from(INPUT + 24), &comparison.to_le_bytes())
                .unwrap();
            memory
                .write(u64::from(INPUT + 32), &3_f64.to_le_bytes())
                .unwrap();
            let mut code = vec![0xdc, 0x15];
            code.extend((INPUT + 24).to_le_bytes());
            code.extend([0xdc, 0x35]);
            code.extend((INPUT + 32).to_le_bytes());
            code.extend([0xdf, 0xe0, 0xd9, constant, 0xdf, 0xe0]);
            replace_code(&mut memory, cpu.eip, &code);
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_2a23 | condition);
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_2023 | condition);
        }
    }
}

#[test]
fn other_constants_forbidden_prefixes_and_truncated_fetch_remain_atomic() {
    for constant in [0xee, 0xe8] {
        for code in [
            vec![0xd9, 0xe9],
            vec![0xd9, 0xea],
            vec![0xd9, 0xeb],
            vec![0xd9, 0xec],
            vec![0xd9, 0xed],
            vec![0xf2, 0xd9, constant],
            vec![0xf3, 0xd9, constant],
            vec![0xf0, 0xd9, constant],
        ] {
            let (mut cpu, mut memory) = load(&[7.0], constant);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            replace_code(&mut memory, cpu.eip, &code);
            let reason = if code[0] == 0xf0 {
                StopReason::InvalidInstruction
            } else {
                StopReason::UnsupportedInstruction
            };
            refuses(&mut cpu, &mut memory, &reason);
        }
        let (mut cpu, mut memory) = load(&[7.0], constant);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        memory
            .map_zeroed(0x5000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x5000_0fff, &[0xd9]).unwrap();
        memory
            .protect(0x5000_0000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        cpu.eip = 0x5000_0fff;
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        assert!(matches!(run.reason, StopReason::MemoryFault(_)));
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
    }
}
