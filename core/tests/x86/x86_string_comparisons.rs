use super::executable;
use super::string_compare_executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

#[test]
fn authored_comparisons_agree_whole_and_per_element() {
    for budget in [1, 100] {
        let mut image = load_pe32(&string_compare_executable::pe32(), 3).unwrap();
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
        assert_eq!(cpu.register(Register32::Ebx), 1);
        assert_eq!(cpu.register(Register32::Ebp), 1);
        assert_eq!(cpu.register(Register32::Ecx), 1);
        assert_eq!(cpu.register(Register32::Esi), 0x0040_20c4);
        assert_eq!(cpu.register(Register32::Edi), 0x0040_20c4);
        assert_eq!(cpu.eflags, 0x46);
    }
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

fn encoding(width: u32, prefix: Option<u8>) -> Vec<u8> {
    let mut bytes: Vec<_> = prefix.into_iter().collect();
    if width == 2 {
        bytes.push(0x66);
    }
    bytes.push(if width == 1 { 0xa6 } else { 0xa7 });
    bytes
}

#[test]
fn scalar_comparisons_match_subtraction_oracle_and_preserve_unrelated_state() {
    for width in [1, 2, 4] {
        let code = encoding(width, None);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mask = u32::MAX >> (32 - width * 8);
        let values = [0, 1, 15, 16, 127, 128, mask / 2, mask / 2 + 1, mask];
        for left in values {
            for right in values {
                image
                    .memory
                    .write(0x0040_2081, &left.to_le_bytes()[..width as usize])
                    .unwrap();
                image
                    .memory
                    .write(0x0040_2181, &right.to_le_bytes()[..width as usize])
                    .unwrap();
                for reverse in [false, true] {
                    let mut cpu = Cpu32::new(image.entry_point);
                    cpu.set_register(Register32::Eax, 0x1234_5678);
                    cpu.set_register(Register32::Ecx, 7);
                    cpu.set_register(Register32::Esi, 0x0040_2081);
                    cpu.set_register(Register32::Edi, 0x0040_2181);
                    cpu.eflags = if reverse { 0xced7 } else { 0xcad7 };
                    let step = if reverse { width.wrapping_neg() } else { width };
                    let mut expected = cpu;
                    expected.eip += u32::try_from(code.len()).unwrap();
                    expected.set_register(Register32::Esi, 0x0040_2081_u32.wrapping_add(step));
                    expected.set_register(Register32::Edi, 0x0040_2181_u32.wrapping_add(step));
                    expected.eflags = flags(left, right, width, cpu.eflags);
                    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                    assert_eq!(cpu, expected, "{width}/{left}/{right}/{reverse}");
                }
            }
        }
    }
}

#[test]
fn repeats_use_new_comparison_and_remaining_count_in_both_directions() {
    for width in [1, 2, 4_u32] {
        for prefix in [0xf2, 0xf3] {
            let code = encoding(width, Some(prefix));
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            for reverse in [false, true] {
                let step = if reverse { width.wrapping_neg() } else { width };
                let left = [1_u32, 3, 5];
                let right = if prefix == 0xf2 {
                    [2_u32, 4, 5]
                } else {
                    [1, 3, 7]
                };
                for index in 0..3_u32 {
                    for (base, value) in [
                        (0x0040_2080_u32, left[index as usize]),
                        (0x0040_2180, right[index as usize]),
                    ] {
                        image
                            .memory
                            .write(
                                u64::from(base.wrapping_add(step.wrapping_mul(index))),
                                &value.to_le_bytes()[..width as usize],
                            )
                            .unwrap();
                    }
                }
                for count in [0, 1, 2, 3, 8_u32] {
                    for initial_zero in [false, true] {
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_register(Register32::Ecx, count);
                        cpu.set_register(Register32::Esi, 0x0040_2080);
                        cpu.set_register(Register32::Edi, 0x0040_2180);
                        cpu.eflags = 2
                            | if reverse { 0x400 } else { 0 }
                            | if initial_zero { 0x40 } else { 0 };
                        let elements = count.min(3);
                        let mut expected = cpu;
                        expected.eip += u32::try_from(code.len()).unwrap();
                        expected.set_register(Register32::Ecx, count - elements);
                        expected.set_register(
                            Register32::Esi,
                            0x0040_2080_u32.wrapping_add(step.wrapping_mul(elements)),
                        );
                        expected.set_register(
                            Register32::Edi,
                            0x0040_2180_u32.wrapping_add(step.wrapping_mul(elements)),
                        );
                        if elements != 0 {
                            let i = elements as usize - 1;
                            expected.eflags = flags(left[i], right[i], width, cpu.eflags);
                        }
                        let run = cpu.run(&mut image.memory, u64::from(elements.max(1)));
                        assert_eq!(run.reason, StopReason::InstructionLimit);
                        assert_eq!(cpu, expected);
                    }
                }
            }
        }
    }
}

#[test]
fn either_operand_fault_preserves_completed_progress_and_can_resume() {
    for width in [1, 2, 4_u32] {
        for prefix in [0xf2, 0xf3] {
            for source_fault in [false, true] {
                for budget in [1, 100] {
                    let code = encoding(width, Some(prefix));
                    let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
                    image
                        .memory
                        .map_zeroed(0x8000, 4096, Permissions::READ_WRITE)
                        .unwrap();
                    let start = 0x9000 - width * 2;
                    let (source, destination) = if source_fault {
                        (start, 0x0040_2080)
                    } else {
                        (0x0040_2080, start)
                    };
                    for i in 0..2 {
                        image
                            .memory
                            .write(
                                u64::from(source + i * width),
                                &1_u32.to_le_bytes()[..width as usize],
                            )
                            .unwrap();
                        let right = if prefix == 0xf2 { 2_u32 } else { 1 };
                        image
                            .memory
                            .write(
                                u64::from(destination + i * width),
                                &right.to_le_bytes()[..width as usize],
                            )
                            .unwrap();
                    }
                    if prefix == 0xf3 {
                        image
                            .memory
                            .write(
                                u64::from(0x0040_2080 + 2 * width),
                                &1_u32.to_le_bytes()[..width as usize],
                            )
                            .unwrap();
                    }
                    let mut cpu = Cpu32::new(image.entry_point);
                    cpu.set_register(Register32::Ecx, u32::MAX);
                    cpu.set_register(Register32::Esi, source);
                    cpu.set_register(Register32::Edi, destination);
                    cpu.eflags = 0xcad7;
                    let mut steps = 0;
                    loop {
                        let run = cpu.run(&mut image.memory, budget);
                        steps += run.instructions;
                        if matches!(run.reason, StopReason::MemoryFault(_)) {
                            break;
                        }
                        assert_eq!(run.reason, StopReason::InstructionLimit);
                        assert_eq!(cpu.eflags, 0xcad7);
                        assert!(steps <= 2);
                    }
                    assert_eq!(steps, 2);
                    assert_eq!(cpu.eip, image.entry_point);
                    assert_eq!(cpu.register(Register32::Ecx), u32::MAX - 2);
                    assert_eq!(cpu.register(Register32::Esi), source + 2 * width);
                    assert_eq!(cpu.register(Register32::Edi), destination + 2 * width);
                    assert_eq!(cpu.eflags, 0xcad7);
                    let before = cpu;
                    assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
                    assert_eq!(
                        cpu.run_until(&mut image.memory, 1, |_| true).reason,
                        StopReason::Intercepted
                    );
                    assert_eq!(cpu, before);
                    image
                        .memory
                        .map_zeroed(0x9000, 4096, Permissions::READ)
                        .unwrap();
                    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                    assert_eq!(
                        cpu.eip,
                        image.entry_point + u32::try_from(code.len()).unwrap()
                    );
                    let pair = if prefix == 0xf2 {
                        (0, 0)
                    } else if source_fault {
                        (0, 1)
                    } else {
                        (1, 0)
                    };
                    assert_eq!(cpu.eflags, flags(pair.0, pair.1, width, 0xcad7));
                }
            }
        }
    }
}

#[test]
fn zero_count_and_invalid_encodings_do_not_touch_either_operand() {
    for width in [1, 2, 4] {
        for prefix in [0xf2, 0xf3] {
            let code = encoding(width, Some(prefix));
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Esi, u32::MAX);
            cpu.set_register(Register32::Edi, 0x6000_0000);
            cpu.eflags = 0xced7;
            let mut expected = cpu;
            expected.eip += u32::try_from(code.len()).unwrap();
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            assert_eq!(cpu, expected);
        }
    }
    for code in [&[0x67, 0xf3, 0xa6][..], &[0x65, 0xa7], &[0xf0, 0xa6]] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let run = cpu.run(&mut image.memory, 1);
        assert!(matches!(
            run.reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn fs_applies_only_to_source_and_readonly_overlapping_inputs_are_valid() {
    for width in [1, 2, 4_u32] {
        let mut code = vec![0x64];
        code.extend(encoding(width, None));
        let mut image = load_pe32(&executable::pe32(&code), 6).unwrap();
        image
            .memory
            .write(0x0040_2081, &7_u32.to_le_bytes())
            .unwrap();
        image
            .memory
            .protect(0x0040_2000, 4096, Permissions::READ)
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(0x0040_2091);
        cpu.set_register(Register32::Esi, 0xffff_fff0);
        cpu.set_register(Register32::Edi, 0x0040_2081);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu.eflags, 0x46);
        assert_eq!(cpu.register(Register32::Esi), 0xffff_fff0 + width);
        assert_eq!(cpu.register(Register32::Edi), 0x0040_2081 + width);
    }
}

#[test]
fn guest_span_edges_and_index_wrapping_are_distinct() {
    for width in [1, 2, 4_u32] {
        let code = encoding(width, None);
        let mut image = load_pe32(&executable::pe32(&code), 6).unwrap();
        for base in [0, 0xffff_f000, 0x0040_3000] {
            image
                .memory
                .map_zeroed(base, 4096, Permissions::READ)
                .unwrap();
        }
        for address in [0x0040_2fff, u32::MAX - width + 1, 0] {
            let reverse = address == 0;
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Esi, address);
            cpu.set_register(Register32::Edi, address);
            cpu.eflags = if reverse { 0x402 } else { 2 };
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            let expected = if reverse {
                address.wrapping_sub(width)
            } else {
                address.wrapping_add(width)
            };
            assert_eq!(cpu.register(Register32::Esi), expected);
            assert_eq!(cpu.register(Register32::Edi), expected);
        }
        if width > 1 {
            for bad_source in [true, false] {
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Esi, if bad_source { u32::MAX } else { 0 });
                cpu.set_register(Register32::Edi, if bad_source { 0 } else { u32::MAX });
                let before = cpu;
                assert!(matches!(
                    cpu.run(&mut image.memory, 1).reason,
                    StopReason::MemoryFault(_)
                ));
                assert_eq!(cpu, before);
            }
        }
    }
}
