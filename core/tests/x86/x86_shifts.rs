use super::executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

fn expected(value: u32, bits: u32, operation: u8, count: u8, flags: u32) -> (u32, u32) {
    let count = u32::from(count & 31);
    if count == 0 {
        return (value, flags);
    }
    let value = u64::from(value);
    let sign = value & (1 << (bits - 1)) != 0;
    let mask = (1_u64 << bits) - 1;
    let result = match operation {
        4 => (value << count) & mask,
        5 => value >> count,
        7 => {
            let signed = i64::try_from(value).unwrap() - if sign { 1_i64 << bits } else { 0 };
            (signed >> count).cast_unsigned() & mask
        }
        _ => unreachable!(),
    };
    let carry = match operation {
        4 if count < bits => value & (1 << (bits - count)) != 0,
        5 if count < bits => value & (1 << (count - 1)) != 0,
        7 if count >= bits => sign,
        7 => value & (1 << (count - 1)) != 0,
        _ => false,
    };
    let negative = result & (1 << (bits - 1)) != 0;
    let overflow = count == 1
        && match operation {
            4 => negative != carry,
            5 => sign,
            _ => false,
        };
    let flags = (flags & !0x8d5)
        | u32::from(carry)
        | (u32::from(result.to_le_bytes()[0].count_ones() % 2 == 0) << 2)
        | (u32::from(result == 0) << 6)
        | (u32::from(negative) << 7)
        | (u32::from(overflow) << 11);
    (u32::try_from(result).unwrap(), flags)
}

#[test]
fn documented_shift_forms_match_widened_math_and_flag_policy() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let sign = 1 << (bits - 1);
        for operation in [4, 5, 7] {
            for value in [0, 1, 3, sign - 1, sign, sign + 1, mask] {
                for source in 0..3 {
                    let counts: &[u8] = if source == 0 {
                        &[1]
                    } else {
                        &[0, 1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 255]
                    };
                    for &count in counts {
                        for memory_destination in [false, true] {
                            let mut code = Vec::new();
                            if bits == 16 {
                                code.push(0x66);
                            }
                            code.push([0xd0, 0xd2, 0xc0][source] + u8::from(bits != 8));
                            code.push((operation << 3) | if memory_destination { 5 } else { 0xc0 });
                            if memory_destination {
                                code.extend_from_slice(&0x0040_2001_u32.to_le_bytes());
                            }
                            if source == 2 {
                                code.push(count);
                            }
                            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                            let bytes = usize::try_from(bits / 8).unwrap();
                            image.memory.write(0x0040_2000, &[0xa5; 6]).unwrap();
                            image
                                .memory
                                .write(0x0040_2001, &value.to_le_bytes()[..bytes])
                                .unwrap();
                            let mut cpu = Cpu32::new(image.entry_point);
                            cpu.set_register(Register32::Eax, (0x1234_5678 & !mask) | value);
                            cpu.set_register(Register32::Ecx, 0x1234_5600 | u32::from(count));
                            cpu.eflags = 0xced7;
                            let (result, flags) =
                                expected(value, bits, operation, count, cpu.eflags);
                            let mut wanted = cpu;
                            wanted.eip += u32::try_from(code.len()).unwrap();
                            wanted.eflags = flags;
                            if !memory_destination {
                                wanted.set_register(
                                    Register32::Eax,
                                    (cpu.register(Register32::Eax) & !mask) | result,
                                );
                            }
                            let run = cpu.run(&mut image.memory, 1);
                            assert_eq!(run.reason, StopReason::InstructionLimit, "{code:x?}");
                            assert_eq!(run.instructions, 1);
                            assert_eq!(
                                cpu, wanted,
                                "bits={bits} operation={operation} count={count} value={value:x}"
                            );
                            let mut output = [0; 6];
                            image.memory.read(0x0040_2000, &mut output).unwrap();
                            assert_eq!(output[0], 0xa5);
                            assert!(output[bytes + 1..].iter().all(|&byte| byte == 0xa5));
                            assert_eq!(
                                &output[1..=bytes],
                                &if memory_destination { result } else { value }.to_le_bytes()
                                    [..bytes]
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn high_bytes_cl_alias_and_fs_memory_have_step_parity() {
    let code = [
        0xd0, 0xec, // shr ah,1
        0xd3, 0xe1, // shl ecx,cl
        0x64, 0x66, 0xc1, 0x3d, 0, 0, 0, 0, 3, // sar word fs:[0],3
        0xcc,
    ];
    let mut whole = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut stepped = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.set_register(Register32::Eax, 0x1234_817f);
    cpu.set_register(Register32::Ecx, 0x1000_0003);
    cpu.set_fs_base(0x0040_2ffe);
    let mut other = cpu;
    for memory in [&mut whole.memory, &mut stepped.memory] {
        memory
            .write(0x0040_2ffe, &0x8001_u16.to_le_bytes())
            .unwrap();
    }
    assert_eq!(
        cpu.run(&mut whole.memory, 10).reason,
        StopReason::Breakpoint
    );
    for _ in 0..3 {
        assert_eq!(
            other.run(&mut stepped.memory, 1).reason,
            StopReason::InstructionLimit
        );
    }
    assert_eq!(
        other.run(&mut stepped.memory, 1).reason,
        StopReason::Breakpoint
    );
    assert_eq!(other, cpu);
    assert_eq!(cpu.register(Register32::Eax), 0x1234_407f);
    assert_eq!(cpu.register(Register32::Ecx), 0x8000_0018);
    assert_eq!(cpu.eflags & 0x8d5, 0x84);
    for memory in [&whole.memory, &stepped.memory] {
        let mut result = [0; 2];
        memory.read(0x0040_2ffe, &mut result).unwrap();
        assert_eq!(result, 0xf000_u16.to_le_bytes());
    }
}

#[test]
fn shift_memory_faults_preserve_state_even_for_masked_zero_counts() {
    for count in [0, 1, 32] {
        for address in [0x0040_2ffe_u32, 0xffff_fffe] {
            let mut code = vec![0xc1, 0x25];
            code.extend_from_slice(&address.to_le_bytes());
            code.push(count);
            let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
            if address == 0x0040_2ffe {
                image
                    .memory
                    .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
                    .unwrap();
            } else {
                image
                    .memory
                    .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
            }
            image.memory.write(u64::from(address), &[0xff; 2]).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.eflags = 0xced7;
            let before = cpu;
            let run = cpu.run(&mut image.memory, 1);
            assert!(matches!(run.reason, StopReason::MemoryFault(_)));
            assert_eq!(run.instructions, 0);
            assert_eq!(cpu, before);
            let mut result = [0; 2];
            image.memory.read(u64::from(address), &mut result).unwrap();
            assert_eq!(result, [0xff; 2]);
        }
    }
}

#[test]
fn undocumented_aliases_rotates_and_unsupported_prefixes_still_stop() {
    for code in [
        &[0xc1, 0xf0, 1][..],
        &[0xd0, 0xf0],
        &[0x66, 0xd3, 0xf0],
        &[0xc1, 0xc0, 1],
        &[0x0f, 0xa4, 0xd8, 1],
        &[0x67, 0xd1, 0x20],
        &[0xf3, 0xd1, 0xe0],
        &[0x2e, 0xd1, 0xe0],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let mut image = load_pe32(&executable::pe32(&[0xf0, 0xd1, 0x20]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::InvalidInstruction
    );
}
