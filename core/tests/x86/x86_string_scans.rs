use super::executable;
use super::string_scan_executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

fn encoding(width: u32, prefix: Option<u8>) -> Vec<u8> {
    let mut bytes: Vec<_> = prefix.into_iter().collect();
    if width == 2 {
        bytes.push(0x66);
    }
    bytes.push(if width == 1 { 0xae } else { 0xaf });
    bytes
}

fn flags(left: u32, right: u32, width: u32, previous: u32) -> u32 {
    let modulus = 1_i64 << (width * 8);
    let sign = modulus / 2;
    let signed = |value: u32| {
        if i64::from(value) >= sign {
            i64::from(value) - modulus
        } else {
            i64::from(value)
        }
    };
    let result = (i64::from(left) - i64::from(right)).rem_euclid(modulus);
    let difference = signed(left) - signed(right);
    (previous & !0x8d5)
        | u32::from(left < right)
        | (u32::from((result & 255).count_ones().is_multiple_of(2)) << 2)
        | (u32::from(left & 15 < right & 15) << 4)
        | (u32::from(result == 0) << 6)
        | (u32::from(result >= sign) << 7)
        | (u32::from(difference < -sign || difference >= sign) << 11)
}

#[test]
fn scalar_scans_match_independent_subtraction_oracle_and_preserve_other_state() {
    for width in [1, 2, 4] {
        let code = encoding(width, None);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mask = u32::MAX >> (32 - width * 8);
        let values: Vec<_> = if width == 1 {
            (0..256).collect()
        } else {
            vec![0, 1, 15, 16, 127, 128, mask / 2, mask / 2 + 1, mask]
        };
        for &left in &values {
            for &right in &values {
                image
                    .memory
                    .write(0x0040_2081, &right.to_le_bytes()[..width as usize])
                    .unwrap();
                for reverse in [false, true] {
                    let mut cpu = Cpu32::new(image.entry_point);
                    cpu.set_register(Register32::Eax, (0xa5a5_5a5a & !mask) | left);
                    cpu.set_register(Register32::Ecx, if left & 1 == 0 { 0 } else { 7 });
                    cpu.set_register(Register32::Esi, 0x1234_5678);
                    cpu.set_register(Register32::Edi, 0x0040_2081);
                    cpu.eflags = if reverse { 0xced7 } else { 0xcad7 };
                    let mut expected = cpu;
                    expected.eip += u32::try_from(code.len()).unwrap();
                    expected.set_register(
                        Register32::Edi,
                        if reverse {
                            0x0040_2081 - width
                        } else {
                            0x0040_2081 + width
                        },
                    );
                    expected.eflags = flags(left, right, width, cpu.eflags);
                    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                    assert_eq!(cpu, expected, "width={width} left={left} right={right}");
                }
            }
        }
    }
}

#[test]
fn repeats_use_new_equality_and_remaining_count_in_both_directions() {
    for width in [1, 2, 4_u32] {
        for prefix in [0xf2, 0xf3] {
            let code = encoding(width, Some(prefix));
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            for reverse in [false, true] {
                let step = if reverse { width.wrapping_neg() } else { width };
                let values: [u32; 3] = if prefix == 0xf2 { [2, 2, 1] } else { [1, 1, 2] };
                for (index, value) in values.into_iter().enumerate() {
                    let address = 0x0040_2080_u32
                        .wrapping_add(step.wrapping_mul(u32::try_from(index).unwrap()));
                    image
                        .memory
                        .write(u64::from(address), &value.to_le_bytes()[..width as usize])
                        .unwrap();
                }
                for count in [0, 1, 2, 3, 8_u32] {
                    for initial_zero in [false, true] {
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_register(Register32::Eax, 1);
                        cpu.set_register(Register32::Ecx, count);
                        cpu.set_register(Register32::Edi, 0x0040_2080);
                        cpu.eflags = 2
                            | if reverse { 0x400 } else { 0 }
                            | if initial_zero { 0x40 } else { 0 };
                        let initial_flags = cpu.eflags;
                        let elements = count.min(3);
                        let result = cpu.run(&mut image.memory, u64::from(elements.max(1)));
                        assert_eq!(result.reason, StopReason::InstructionLimit);
                        assert_eq!(result.instructions, u64::from(elements.max(1)));
                        assert_eq!(
                            cpu.eip,
                            image.entry_point + u32::try_from(code.len()).unwrap()
                        );
                        assert_eq!(cpu.register(Register32::Ecx), count - elements);
                        assert_eq!(
                            cpu.register(Register32::Edi),
                            0x0040_2080_u32.wrapping_add(step.wrapping_mul(elements))
                        );
                        assert_eq!(
                            cpu.eflags,
                            if elements == 0 {
                                initial_flags
                            } else {
                                flags(1, values[elements as usize - 1], width, initial_flags)
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn budgets_and_faults_preserve_completed_progress_and_defer_flags_until_termination() {
    for width in [1, 2, 4_u32] {
        for reverse in [false, true] {
            for budget in [1, 100] {
                let code = encoding(width, Some(0xf2));
                let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
                image
                    .memory
                    .map_zeroed(0x8000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                let start = if reverse {
                    0x8000 + width
                } else {
                    0x9000 - width * 2
                };
                let step = if reverse { width.wrapping_neg() } else { width };
                for index in 0..2 {
                    let address = start.wrapping_add(step.wrapping_mul(index));
                    image
                        .memory
                        .write(u64::from(address), &1_u32.to_le_bytes()[..width as usize])
                        .unwrap();
                }
                let fault_address = start.wrapping_add(step.wrapping_mul(2));
                let page = u64::from(fault_address & !0xfff);
                if budget == 100 {
                    image
                        .memory
                        .map_zeroed(page, PAGE_SIZE, Permissions::NONE)
                        .unwrap();
                }
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Ecx, u32::MAX);
                cpu.set_register(Register32::Edi, start);
                cpu.eflags = 0xcad7 | if reverse { 0x400 } else { 0 };
                let original_flags = cpu.eflags;
                let mut total = 0;
                loop {
                    let result = cpu.run(&mut image.memory, budget);
                    total += result.instructions;
                    if matches!(result.reason, StopReason::MemoryFault(_)) {
                        break;
                    }
                    assert_eq!(result.reason, StopReason::InstructionLimit);
                    assert_eq!(cpu.eflags, original_flags);
                    assert!(total <= 2);
                }
                assert_eq!(total, 2);
                assert_eq!(cpu.eip, image.entry_point);
                assert_eq!(cpu.register(Register32::Ecx), u32::MAX - 2);
                assert_eq!(cpu.register(Register32::Edi), fault_address);
                assert_eq!(cpu.eflags, original_flags);
                let before = cpu;
                assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
                assert_eq!(
                    cpu.run_until(&mut image.memory, 1, |_| true).reason,
                    StopReason::Intercepted
                );
                assert_eq!(cpu, before);
                if budget == 100 {
                    image
                        .memory
                        .protect(page, PAGE_SIZE, Permissions::READ)
                        .unwrap();
                } else {
                    image
                        .memory
                        .map_zeroed(page, PAGE_SIZE, Permissions::READ)
                        .unwrap();
                }
                assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                assert_eq!(
                    cpu.eip,
                    image.entry_point + u32::try_from(code.len()).unwrap()
                );
                assert_eq!(cpu.register(Register32::Ecx), u32::MAX - 3);
                assert_eq!(cpu.eflags, flags(0, 0, width, original_flags));
            }
        }
    }
}

#[test]
fn zero_count_skips_memory_but_not_encoding_validation() {
    for code in [vec![0xf2, 0xae], vec![0xf3, 0x66, 0xaf], vec![0xf2, 0xaf]] {
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Edi, u32::MAX);
        cpu.eflags = 0xced7;
        let mut expected = cpu;
        expected.eip += u32::try_from(code.len()).unwrap();
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu, expected);
    }
    for code in [
        &[0xf2, 0x67, 0xae][..],
        &[0xf3, 0xac],
        &[0xf2, 0xa5],
        &[0x65, 0xae],
        &[0xf0, 0xae],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(
            result.reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn exact_memory_width_flat_es_and_index_wrapping_are_preserved() {
    for width in [1, 2, 4_u32] {
        let mut code = encoding(width, None);
        code.insert(0, 0x64);
        let mut image = load_pe32(&executable::pe32(&code), 6).unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        image
            .memory
            .map_zeroed(0, PAGE_SIZE, Permissions::READ)
            .unwrap();
        image
            .memory
            .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
            .unwrap();
        for (address, reverse) in [
            (0x0040_2fff, false),
            (u32::MAX - width + 1, false),
            (0, true),
        ] {
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_fs_base(0x7000_0000);
            cpu.set_register(Register32::Edi, address);
            cpu.eflags = if reverse { 0x402 } else { 2 };
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            assert_eq!(
                cpu.register(Register32::Edi),
                if reverse {
                    address.wrapping_sub(width)
                } else {
                    address.wrapping_add(width)
                }
            );
        }
        if width > 1 {
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Edi, u32::MAX);
            let before = cpu;
            assert!(matches!(
                cpu.run(&mut image.memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        }
    }
}

#[test]
fn authored_scans_agree_whole_and_per_element() {
    for budget in [1, 100] {
        let mut image = load_pe32(&string_scan_executable::pe32(), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut steps = 0;
        loop {
            let result = cpu.run(&mut image.memory, budget);
            steps += result.instructions;
            if result.reason != StopReason::InstructionLimit {
                assert_eq!(result.reason, StopReason::Breakpoint);
                break;
            }
            assert!(steps < 100);
        }
        assert_eq!(steps, 17);
        assert_eq!(cpu.register(Register32::Eax), 0x4242);
        assert_eq!(cpu.register(Register32::Ebx), 3);
        assert_eq!(cpu.register(Register32::Ecx), 0);
        assert_eq!(cpu.register(Register32::Edi), 0x0040_2096);
        assert_eq!(cpu.eflags, 2);
    }
}
