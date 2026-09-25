#[path = "support/executable.rs"]
mod executable;
#[path = "support/nan_compare_cases.rs"]
mod nan_compare_cases;

#[test]
fn loaded_nan_comparisons_stop_without_host_panic_or_partial_execution() {
    nan_compare_cases::nan_top_refuses_comparison_atomically();
}

use nan_compare_cases::{INPUT, OUTPUT, SOURCE, load, read};
use ring3_core::execution::{Permissions, Register32, StopReason};

#[test]
fn source_faults_keep_precedence_and_repair_reaches_controlled_nan_stop() {
    for (opcode, mode) in [(0xd8, 0x15), (0xdc, 0x15), (0xd8, 0x1d), (0xdc, 0x1d)] {
        let (mut cpu, mut memory) = load(0x7ff8_0000_0000_0001, opcode, mode, 0x6000_0000);
        let entry = cpu.eip;
        cpu.eip += 6;
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        cpu.eip = entry;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        cpu.set_x87_control_word(0x027e);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        cpu.set_x87_control_word(0x027f);
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        assert!(matches!(run.reason, StopReason::MemoryFault(_)));
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
        memory
            .map_zeroed(0x6000_0000, 4096, Permissions::READ_WRITE)
            .unwrap();
        let source = read(&memory, SOURCE);
        memory.write(0x6000_0000, &source.to_le_bytes()).unwrap();
        memory
            .protect(0x6000_0000, 4096, Permissions::READ)
            .unwrap();
        let run = cpu.run(&mut memory, 1);
        assert_eq!(run.reason, StopReason::UnsupportedInstruction);
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
        assert_eq!(read(&memory, OUTPUT), 0x5555_5555_5555_5555);
        assert_eq!(read(&memory, 0x6000_0000), source);
        memory
            .protect(0x6000_0000, 4096, Permissions::NONE)
            .unwrap();
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
    }
}

#[test]
fn rejected_nan_can_be_stored_and_replaced_before_finite_comparison_retry() {
    for bits in [0xfff8_0000_0000_0011, 0xfff0_0000_0000_0001] {
        for (opcode, mode) in [(0xd8, 0x15), (0xdc, 0x15), (0xd8, 0x1d), (0xdc, 0x1d)] {
            let (mut cpu, mut memory) = load(bits, opcode, mode, SOURCE);
            let entry = cpu.eip;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let comparison = cpu.eip;
            let before = cpu;
            assert_eq!(
                cpu.run(&mut memory, 1).reason,
                StopReason::UnsupportedInstruction
            );
            assert_eq!(cpu, before);
            cpu.eip = comparison + 6;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            assert_eq!(read(&memory, OUTPUT), bits | (1 << 51));
            memory
                .write(u64::from(INPUT), &1_f64.to_le_bytes())
                .unwrap();
            cpu.eip = entry;
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            cpu.eip = comparison + 12;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let invalid = u32::from(bits & (1 << 51) == 0);
            let top = if mode == 0x15 { 0x3800 } else { 0 };
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_4000 | top | invalid);
            assert_eq!(cpu.eflags, 0xced7);
        }
    }
}
