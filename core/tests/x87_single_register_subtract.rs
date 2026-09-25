#[path = "support/executable.rs"]
mod executable;
#[path = "support/single_register_subtract_cases.rs"]
mod single_register_subtract_cases;

use ring3_core::execution::{Permissions, Register32, StopReason};
use single_register_subtract_cases::{OUTPUT, load, read};

#[test]
fn single_register_subtraction_preserves_sources_and_rounds_once_across_budgets() {
    single_register_subtract_cases::arithmetic();
}

#[test]
fn self_source_is_positive_zero_and_exact_subtraction_preserves_sticky_precision() {
    for value in [0.0, -0.0, 3.0, -3.0] {
        let (mut cpu, mut memory) = load(&[value], 0);
        assert_eq!(cpu.run(&mut memory, 10).reason, StopReason::Breakpoint);
        assert_eq!(read(&memory, OUTPUT), 0);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_3800);
    }
    let (mut cpu, mut memory) = load(&[f64::MIN_POSITIVE, 1.0], 1);
    assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_3220);
    let instruction = cpu.eip - 4;
    memory
        .protect(0x0040_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(u64::from(instruction), &[0xd8, 0xe0]).unwrap();
    memory
        .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    cpu.eip = instruction;
    assert_eq!(cpu.run(&mut memory, 10).reason, StopReason::Breakpoint);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_3020);
    assert_eq!(read(&memory, OUTPUT), 0);
    assert_eq!(read(&memory, OUTPUT + 8), f64::MIN_POSITIVE.to_bits());
}

#[test]
fn register_arithmetic_does_not_access_guest_data_or_change_unrelated_cpu_state() {
    let (mut cpu, mut memory) = load(&[3.0, 3.0], 1);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    let (mut expected, mut other) = load(&[3.0, 0.0], 1);
    assert_eq!(expected.run(&mut other, 2).instructions, 2);
    expected.eip += 2;
    memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let run = cpu.run(&mut memory, 1);
    assert_eq!(run.reason, StopReason::InstructionLimit);
    assert_eq!(run.instructions, 1);
    assert_eq!(cpu, expected);
}

#[test]
fn unsupported_controls_and_result_ranges_refuse_atomically_and_control_can_be_repaired() {
    for control in [0x017f, 0x037f, 0x047f, 0x087f, 0x0c7f, 0x007e, 0x005f] {
        let (mut cpu, mut memory) = load(&[2.0, 5.0], 1);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        cpu.set_x87_control_word(control);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(read(&memory, OUTPUT), 0);
        cpu.set_x87_control_word(0x007f);
        assert_eq!(cpu.run(&mut memory, 10).reason, StopReason::Breakpoint);
        assert_eq!(read(&memory, OUTPUT), 3.0_f64.to_bits());
    }
    for (source, top) in [
        (-f64::MAX, f64::MAX),
        (0.0, f64::from(f32::MAX) * 2.0),
        (
            f64::from(f32::MIN_POSITIVE) * 0.5,
            f64::from(f32::MIN_POSITIVE),
        ),
        (0.0, f64::MIN_POSITIVE),
        (f64::NAN, 0.0),
    ] {
        let (mut cpu, mut memory) = load(&[source, top], 1);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(read(&memory, OUTPUT), 0);
    }
}

#[test]
fn absent_register_sources_preserve_the_stack_and_can_be_supplied_before_retry() {
    for (values, index) in [(&[][..], 0), (&[1.0][..], 1), (&[1.0, 2.0][..], 7)] {
        let (mut cpu, mut memory) = load(values, index);
        assert_eq!(
            cpu.run(&mut memory, values.len() as u64).instructions,
            values.len() as u64
        );
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let (mut cpu, mut memory) = load(&[2.0, 5.0], 1);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let skipped_load = cpu.eip;
    cpu.eip += 6;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    cpu.eip = skipped_load;
    assert_eq!(cpu.run(&mut memory, 10).reason, StopReason::Breakpoint);
    assert_eq!(read(&memory, OUTPUT), 3.0_f64.to_bits());
}

#[test]
fn forbidden_prefixes_and_truncated_fetch_leave_loaded_x87_state_unchanged() {
    for (code, reason) in [
        (&[0xf2, 0xd8, 0xe1][..], StopReason::UnsupportedInstruction),
        (&[0xf3, 0xd8, 0xe1][..], StopReason::UnsupportedInstruction),
        (&[0xf0, 0xd8, 0xe1][..], StopReason::InvalidInstruction),
    ] {
        let (mut cpu, mut memory) = load(&[2.0, 5.0], 1);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        memory
            .map_zeroed(0x5000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x5000_0000, code).unwrap();
        memory.write(0x5000_0fff, &[0xd8]).unwrap();
        memory
            .protect(0x5000_0000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        cpu.eip = 0x5000_0000;
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        assert_eq!(run.reason, reason);
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
        cpu.eip = 0x5000_0fff;
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
    }
}
