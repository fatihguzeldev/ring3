#[path = "support/executable.rs"]
mod executable;
#[path = "support/register_compare_pop_cases.rs"]
mod register_compare_pop_cases;

#[test]
fn register_comparisons_pop_once_across_profiles_and_budgets() {
    register_compare_pop_cases::register_comparisons_pop_once_across_profiles_and_budgets();
}

use register_compare_pop_cases::{INPUT, OUTPUT, load, read};
use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason};

fn replace_code(memory: &mut GuestMemory, address: u32, code: &[u8]) {
    memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(u64::from(address), code).unwrap();
    memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
}

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
fn register_source_accesses_no_data_memory_and_matches_existing_compare_state() {
    let (mut cpu, mut memory) = load(&[3., 1.], 1);
    let (mut expected, mut other) = load(&[3., 1.], 1);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(expected.run(&mut other, 2).instructions, 2);
    let mut comparison = vec![0xdc, 0x1d];
    comparison.extend(INPUT.to_le_bytes());
    replace_code(&mut other, expected.eip, &comparison);
    assert_eq!(expected.run(&mut other, 1).instructions, 1);
    expected.eip = cpu.eip + 2;
    let pages = memory.mapped_pages();
    memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu, expected);
    assert_eq!(memory.mapped_pages(), pages);
}

#[test]
fn missing_registers_refuse_atomically_and_added_value_allows_retry() {
    for depth in 0..8_u8 {
        let values = vec![1.; usize::from(depth)];
        let (mut cpu, mut memory) = load(&values, depth);
        assert_eq!(
            cpu.run(&mut memory, u64::from(depth)).instructions,
            u64::from(depth)
        );
        let comparison = cpu.eip;
        refuses(&mut cpu, &mut memory, &StopReason::UnsupportedInstruction);
        memory
            .write(u64::from(INPUT), &1_f64.to_le_bytes())
            .unwrap();
        let mut code = vec![0xdd, 0x05];
        code.extend(INPUT.to_le_bytes());
        replace_code(&mut memory, 0x0040_1200, &code);
        cpu.eip = 0x0040_1200;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        cpu.eip = comparison;
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(
            cpu.register(Register32::Eax),
            0xabcd_4000 | ((u32::from(8 - depth) & 7) << 11)
        );
    }
}

#[test]
fn each_unmasked_exception_refuses_and_control_repair_retries() {
    for bit in 0..6 {
        let (mut cpu, mut memory) = load(&[3., 1.], 1);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        cpu.set_x87_control_word(0x027f & !(1 << bit));
        refuses(&mut cpu, &mut memory, &StopReason::UnsupportedInstruction);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_3900);
    }
}

#[test]
fn unsupported_value_on_either_side_preserves_state_and_finite_replacement_retries() {
    for bits in [
        0x7ff8_0000_0000_0011,
        0xfff8_0000_0000_0011,
        0x7ff0_0000_0000_0001,
        0xfff0_0000_0000_0001,
        1,
        0x8000_0000_0000_0001,
    ] {
        for side in 0..2 {
            let mut values = [1.; 2];
            values[side] = f64::from_bits(bits);
            let (mut cpu, mut memory) = load(&values, 1);
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            let comparison = cpu.eip;
            refuses(&mut cpu, &mut memory, &StopReason::UnsupportedInstruction);
            replace_code(&mut memory, 0x0040_1200, &[0xdd, 0xd8, 0xdd, 0xd8]);
            cpu.eip = 0x0040_1200;
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            memory
                .write(u64::from(INPUT), &1_f64.to_le_bytes())
                .unwrap();
            memory
                .write(u64::from(INPUT + 8), &1_f64.to_le_bytes())
                .unwrap();
            cpu.eip = 0x0040_1000;
            assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
            assert_eq!(cpu.eip, comparison + 4);
            let sticky = if f64::from_bits(bits).is_subnormal() {
                2
            } else {
                u32::from(bits & (1 << 51) == 0)
            };
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_7800 | sticky);
        }
    }
}

#[test]
fn comparisons_clear_roundup_and_replace_conditions_but_keep_sticky_exceptions() {
    for (right, condition) in [(2., 0), (4., 0x100)] {
        let values = [
            f64::from_bits(0x7ff0_0000_0000_0001),
            f64::from_bits(1),
            right,
            10.,
        ];
        let (mut cpu, mut memory) = load(&values, 1);
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        let mut code = vec![0xdc, 0x15];
        code.extend((INPUT + 24).to_le_bytes());
        code.extend([0xdc, 0x35]);
        code.extend((INPUT + 32).to_le_bytes());
        code.extend([0xdf, 0xe0, 0xd8, 0xd9, 0xdf, 0xe0]);
        memory
            .write(u64::from(INPUT + 32), &3_f64.to_le_bytes())
            .unwrap();
        replace_code(&mut memory, cpu.eip, &code);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_6223);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_2823 | condition);
    }
}

#[test]
fn excluded_forms_prefixes_and_truncated_fetch_keep_the_stack_atomic() {
    for code in [
        vec![0xd8, 0xd1],
        vec![0xde, 0xd9],
        vec![0xdc, 0xd9],
        vec![0xde, 0xd1],
        vec![0xf2, 0xd8, 0xd9],
        vec![0xf3, 0xd8, 0xd9],
        vec![0xf0, 0xd8, 0xd9],
    ] {
        let (mut cpu, mut memory) = load(&[3., 1.], 1);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        replace_code(&mut memory, cpu.eip, &code);
        let reason = if code[0] == 0xf0 {
            StopReason::InvalidInstruction
        } else {
            StopReason::UnsupportedInstruction
        };
        refuses(&mut cpu, &mut memory, &reason);
    }
    let (mut cpu, mut memory) = load(&[3., 1.], 1);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    memory
        .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x6000_0fff, &[0xd8]).unwrap();
    memory
        .protect(0x6000_0000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    cpu.eip = 0x6000_0fff;
    let before = cpu;
    let run = cpu.run(&mut memory, 1);
    assert!(matches!(run.reason, StopReason::MemoryFault(_)));
    assert_eq!(run.instructions, 0);
    assert_eq!(cpu, before);
    memory
        .map_zeroed(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x6000_1000, &[0xd9]).unwrap();
    memory
        .protect(0x6000_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu.eip, 0x6000_1001);
}
