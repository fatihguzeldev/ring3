use super::executable;
use super::x87_register_executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2300;

fn transfer(code: &mut Vec<u8>, mode: u8, address: u32) {
    code.extend([0xdd, mode]);
    code.extend(address.to_le_bytes());
}

fn load(code: &[u8]) -> (Cpu32, GuestMemory) {
    let image = load_pe32(&executable::pe32(code), 32).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

#[test]
fn register_load_copies_occupied_source_above_existing_stack() {
    let mut code = Vec::new();
    transfer(&mut code, 0x05, INPUT);
    transfer(&mut code, 0x05, INPUT + 8);
    code.extend([0xd9, 0xc1, 0xdf, 0xe0]);
    for index in 0..3 {
        code.extend([0xd9, 0x1d]);
        code.extend((OUTPUT + index * 8).to_le_bytes());
    }
    for (source, expected_source) in [(2.5_f64, 2.5_f32.to_bits()), (-0.0, (-0.0_f32).to_bits())] {
        let (mut cpu, mut memory) = load(&code);
        cpu.set_x87_control_word(0x007f);
        memory
            .write(u64::from(INPUT), &source.to_le_bytes())
            .unwrap();
        memory
            .write(u64::from(INPUT + 8), &3.0_f64.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x2800);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        for (index, expected) in [expected_source, 3.0_f32.to_bits(), expected_source]
            .into_iter()
            .enumerate()
        {
            let mut bytes = [0; 4];
            memory
                .read(
                    u64::from(OUTPUT) + u64::try_from(index).unwrap() * 8,
                    &mut bytes,
                )
                .unwrap();
            assert_eq!(u32::from_le_bytes(bytes), expected);
        }
    }
}

#[test]
fn register_load_rejects_missing_source_full_stack_and_unmasked_control_atomically() {
    let mut code = Vec::new();
    transfer(&mut code, 0x05, INPUT);
    code.extend([0xd9, 0xc1]);
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &2.5_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    let mut code = Vec::new();
    for _ in 0..8 {
        transfer(&mut code, 0x05, INPUT);
    }
    code.extend([0xd9, 0xc1]);
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &2.5_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 8).instructions, 8);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    let mut code = Vec::new();
    transfer(&mut code, 0x05, INPUT);
    code.extend([0xd9, 0xc0]);
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &2.5_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    cpu.set_x87_control_word(0x027e);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn register_copy_and_pop_run_whole_or_stepwise() {
    for budget in [1, 100] {
        let image = load_pe32(&x87_register_executable::pe32(), 32).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut memory = image.memory;
        let mut count = 0;
        loop {
            let run = cpu.run(&mut memory, budget);
            count += run.instructions;
            if run.reason != StopReason::InstructionLimit {
                assert_eq!(run.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 15);
        }
        assert_eq!(count, 10);
        assert_eq!(cpu.register(Register32::Eax), 0);
        let mut output = [0; 8];
        memory.read(0x0040_21a0, &mut output).unwrap();
        assert_eq!(output, (-0_f64).to_le_bytes());
    }
}

#[test]
fn every_occupied_destination_preserves_bits_and_stack_order() {
    for bits in [
        0_u64,
        (-0_f64).to_bits(),
        f64::MIN_POSITIVE.to_bits(),
        f64::MAX.to_bits(),
    ] {
        for depth in 1_u8..=8 {
            for destination in 0..depth {
                for pop in [false, true] {
                    let mut code = Vec::new();
                    let mut values: Vec<_> = (1..=depth).map(|v| f64::from(v).to_bits()).collect();
                    values[usize::from(depth - 1)] = bits;
                    for index in 0..depth {
                        transfer(&mut code, 0x05, INPUT + u32::from(index) * 8);
                    }
                    code.extend([0xdd, (if pop { 0xd8 } else { 0xd0 }) + destination]);
                    let remaining = depth - u8::from(pop);
                    for index in 0..remaining {
                        transfer(&mut code, 0x1d, OUTPUT + u32::from(index) * 8);
                    }
                    code.extend([0xdf, 0xe0]);
                    let (mut cpu, mut memory) = load(&code);
                    let pages = memory.mapped_pages();
                    for (index, value) in values.iter().enumerate() {
                        memory
                            .write(
                                u64::from(INPUT) + u64::try_from(index).unwrap() * 8,
                                &value.to_le_bytes(),
                            )
                            .unwrap();
                    }
                    assert_eq!(
                        cpu.run(&mut memory, u64::from(depth)).instructions,
                        u64::from(depth)
                    );
                    memory
                        .protect(0x0040_2000, 4096, Permissions::NONE)
                        .unwrap();
                    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                    assert_eq!(cpu.eflags, 0xced7);
                    assert_eq!(cpu.x87_control_word(), 0x027f);
                    memory
                        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
                        .unwrap();
                    values.reverse();
                    values[usize::from(destination)] = bits;
                    if pop {
                        values.remove(0);
                    }
                    assert_eq!(
                        cpu.run(&mut memory, u64::from(remaining) + 1).instructions,
                        u64::from(remaining) + 1
                    );
                    for (index, expected) in values.into_iter().enumerate() {
                        let mut output = [0; 8];
                        memory
                            .read(
                                u64::from(OUTPUT) + u64::try_from(index).unwrap() * 8,
                                &mut output,
                            )
                            .unwrap();
                        assert_eq!(u64::from_le_bytes(output), expected);
                    }
                    assert_eq!(cpu.register(Register32::Eax), 0);
                    assert_eq!(memory.mapped_pages(), pages);
                }
            }
        }
    }
}

#[test]
fn exact_copies_ignore_precision_and_rounding_controls_and_preserve_integer_state() {
    let mut code = Vec::new();
    transfer(&mut code, 0x05, INPUT);
    code.extend([0xdd, 0xd0, 0xdd, 0xd8]);
    for pc in [0, 0x100, 0x200, 0x300] {
        for rc in [0, 0x400, 0x800, 0xc00] {
            let (mut cpu, mut memory) = load(&code);
            let control = pc | rc | 0x7f;
            cpu.set_x87_control_word(control);
            cpu.set_register(Register32::Eax, 0xabcd_1234);
            memory
                .write(u64::from(INPUT), &1.25_f64.to_le_bytes())
                .unwrap();
            let empty = cpu;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let mut expected = cpu;
            expected.eip += 2;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            assert_eq!(cpu, expected);
            let mut expected = empty;
            expected.eip += 10;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            assert_eq!(cpu, expected);
        }
    }
}

#[test]
fn empty_operands_and_unmasked_exceptions_stop_without_mutation() {
    for opcode in [0xd0, 0xd8] {
        for index in 0..8 {
            let mut code = Vec::new();
            transfer(&mut code, 0x05, INPUT);
            code.extend([0xdd, opcode + index]);
            let (mut cpu, mut memory) = load(&code);
            cpu.eip += 6;
            let before = cpu;
            assert_eq!(
                cpu.run(&mut memory, 1).reason,
                StopReason::UnsupportedInstruction
            );
            assert_eq!(cpu, before);
            cpu.eip -= 6;
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            if index != 0 {
                let before = cpu;
                assert_eq!(
                    cpu.run(&mut memory, 1).reason,
                    StopReason::UnsupportedInstruction
                );
                assert_eq!(cpu, before);
            }
            for mask in 0..6 {
                cpu.set_x87_control_word(0x027f & !(1 << mask));
                let before = cpu;
                assert_eq!(
                    cpu.run(&mut memory, 1).reason,
                    StopReason::UnsupportedInstruction
                );
                assert_eq!(cpu, before);
            }
        }
    }
}

#[test]
fn copy_clears_roundup_but_keeps_sticky_precision_and_condition_policy() {
    for opcode in [0xd0, 0xd8] {
        let mut code = Vec::new();
        transfer(&mut code, 0x05, INPUT);
        code.extend([0xdc, 0x3d]);
        code.extend((INPUT + 8).to_le_bytes());
        code.extend([0xdf, 0xe0, 0xdd, opcode, 0xdf, 0xe0]);
        let (mut cpu, mut memory) = load(&code);
        memory
            .write(u64::from(INPUT), &10_f64.to_le_bytes())
            .unwrap();
        memory
            .write(u64::from(INPUT) + 8, &1_f64.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), 0x3a20);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(
            cpu.register(Register32::Eax),
            if opcode == 0xd0 { 0x3820 } else { 0x20 }
        );
    }
}

#[test]
fn zero_budget_fetch_faults_and_unsupported_prefixes_preserve_stack() {
    for suffix in [
        vec![0xdd, 0xd8],
        vec![0xf0, 0xdd, 0xd8],
        vec![0xf3, 0xdd, 0xd0],
        vec![0xdf, 0xd0],
    ] {
        let mut code = Vec::new();
        transfer(&mut code, 0x05, INPUT);
        code.extend(&suffix);
        let (mut cpu, mut memory) = load(&code);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
        assert_eq!(cpu, before);
        memory
            .protect(0x0040_1000, 4096, Permissions::NONE)
            .unwrap();
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
        memory
            .protect(0x0040_1000, 4096, Permissions::READ_EXECUTE)
            .unwrap();
        let result = cpu.run(&mut memory, 1);
        if suffix == [0xdd, 0xd8] {
            assert_eq!(result.instructions, 1);
        } else {
            assert!(matches!(
                result.reason,
                StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
            ));
            assert_eq!(cpu, before);
        }
    }
}

#[test]
fn register_stores_keep_existing_comparison_condition_bits() {
    for (right, condition) in [(2_f64, 0x100), (1_f64, 0x4000)] {
        for opcode in [0xd0, 0xd8] {
            let mut code = Vec::new();
            transfer(&mut code, 0x05, INPUT);
            code.extend([0xdc, 0x15]);
            code.extend((INPUT + 8).to_le_bytes());
            code.extend([0xdd, opcode, 0xdf, 0xe0]);
            let (mut cpu, mut memory) = load(&code);
            memory
                .write(u64::from(INPUT), &1_f64.to_le_bytes())
                .unwrap();
            memory
                .write(u64::from(INPUT) + 8, &right.to_le_bytes())
                .unwrap();
            assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
            let top = if opcode == 0xd0 { 0x3800 } else { 0 };
            assert_eq!(cpu.register(Register32::Eax), top | condition);
        }
    }
}
