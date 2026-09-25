#[path = "support/executable.rs"]
mod executable;
#[path = "support/integer_division_cases.rs"]
mod integer_division_cases;

use integer_division_cases::{RESULT, SOURCE, TOP, load, result};
use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason};

#[test]
fn signed_quotients_preserve_source_and_match_across_budgets() {
    integer_division_cases::signed_quotients_across_budgets();
}

fn refuses_unchanged(cpu: &mut Cpu32, memory: &mut GuestMemory) {
    let before = *cpu;
    let output = result(memory);
    let run = cpu.run(memory, 1);
    assert_eq!(run.reason, StopReason::UnsupportedInstruction);
    assert_eq!(run.instructions, 0);
    assert_eq!(*cpu, before);
    assert_eq!(result(memory), output);
}

#[test]
fn zero_divisor_control_and_empty_stack_refuse_then_retry() {
    let (mut cpu, mut memory) = load(6.0, 0, SOURCE, false);
    let entry = cpu.eip;
    cpu.eip += 6;
    refuses_unchanged(&mut cpu, &mut memory);
    cpu.eip = entry;
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    refuses_unchanged(&mut cpu, &mut memory);
    memory
        .write(u64::from(SOURCE), &3_i32.to_le_bytes())
        .unwrap();
    for control in [0x007f, 0x017f, 0x037f, 0x067f, 0x0a7f, 0x0e7f, 0x027e] {
        cpu.set_x87_control_word(control);
        refuses_unchanged(&mut cpu, &mut memory);
    }
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(result(&memory), 2.0_f64.to_bits());
}

#[test]
fn precision_is_sticky_and_exact_division_clears_round_up_without_popping() {
    let (mut cpu, mut memory) = load(32.0, 13, SOURCE, false);
    let divide = cpu.eip + 6;
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_3a20);
    memory
        .write(u64::from(SOURCE), &1_i32.to_le_bytes())
        .unwrap();
    cpu.eip = divide;
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_3820);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(result(&memory), 0x4003_b13b_13b1_3b14);
    cpu.eip = divide;
    refuses_unchanged(&mut cpu, &mut memory);
}

#[test]
fn division_preserves_lower_stack_slots_and_other_registers() {
    let (mut cpu, mut memory) = load(8.0, 2, SOURCE, false);
    let entry = cpu.eip;
    for register in [
        Register32::Ebx,
        Register32::Ecx,
        Register32::Edx,
        Register32::Esi,
        Register32::Edi,
        Register32::Esp,
        Register32::Ebp,
    ] {
        cpu.set_register(register, 0x1234_5678);
    }
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    memory.write(u64::from(TOP), &42_f64.to_le_bytes()).unwrap();
    cpu.eip = entry;
    assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
    assert_eq!(result(&memory), 21_f64.to_bits());
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_3000);
    cpu.eip = entry + 14;
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(result(&memory), 8_f64.to_bits());
    for register in [
        Register32::Ebx,
        Register32::Ecx,
        Register32::Edx,
        Register32::Esi,
        Register32::Edi,
        Register32::Esp,
        Register32::Ebp,
    ] {
        assert_eq!(cpu.register(register), 0x1234_5678);
    }
    assert_eq!(cpu.eflags, 0xced7);
}

#[test]
fn bounded_exponent_range_and_wrapping_fs_address_are_preserved() {
    for (top, source) in [
        (f64::MIN_POSITIVE, 1),
        (f64::MIN_POSITIVE, 2),
        (-f64::MIN_POSITIVE, 2),
        (2.0 * f64::MIN_POSITIVE, i32::MAX),
    ] {
        let (mut cpu, mut memory) = load(top, source, SOURCE, false);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        refuses_unchanged(&mut cpu, &mut memory);
    }
    let (mut cpu, mut memory) = load(4.0 * f64::MIN_POSITIVE, 2, 0xffff_ffe0, true);
    cpu.set_fs_base(SOURCE + 0x20);
    assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
    assert_eq!(result(&memory), 0x0020_0000_0000_0000);
    assert_eq!(cpu.fs_base(), SOURCE + 0x20);
}

#[test]
fn missing_and_cross_page_integer_sources_fault_then_retry() {
    for address in [0x6000_0000_u32, 0x0040_2ffe] {
        let (mut cpu, mut memory) = load(6.0, 3, address, false);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        assert!(matches!(run.reason, StopReason::MemoryFault(_)));
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
        let page = if address == 0x6000_0000 {
            0x6000_0000
        } else {
            0x0040_3000
        };
        memory
            .map_zeroed(page, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory
            .write(u64::from(address), &3_i32.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(result(&memory), 2_f64.to_bits());
    }
}

#[test]
fn read_permission_and_address_overflow_fault_without_partial_execution() {
    let (mut cpu, mut memory) = load(6.0, 3, SOURCE, false);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(result(&memory), 2_f64.to_bits());

    let (mut cpu, mut memory) = load(6.0, 3, u32::MAX - 2, false);
    memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.map_zeroed(0, 4096, Permissions::READ_WRITE).unwrap();
    memory.write(u64::from(u32::MAX - 2), &[3, 0, 0]).unwrap();
    memory.write(0, &[0]).unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    assert_eq!(result(&memory), 0);
    let mut bytes = [0; 8];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 8]);
}
