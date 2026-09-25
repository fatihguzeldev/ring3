use super::borrow_executable;
use super::executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

fn oracle(left: u32, right: u32, borrow: u32, bits: u32) -> (u32, u32) {
    let modulus = 1_i64 << bits;
    let sign = modulus / 2;
    let signed = |value: u32| {
        let value = i64::from(value);
        if value >= sign {
            value - modulus
        } else {
            value
        }
    };
    let unsigned = i64::from(left) - i64::from(right) - i64::from(borrow);
    let result = u32::try_from(unsigned.rem_euclid(modulus)).unwrap();
    let signed_result = signed(left) - signed(right) - i64::from(borrow);
    let parity = (0..8).filter(|bit| result & (1 << bit) != 0).count() % 2 == 0;
    let flags = u32::from(unsigned < 0)
        | (u32::from(parity) << 2)
        | (u32::from((left & 15) < (right & 15) + borrow) << 4)
        | (u32::from(result == 0) << 6)
        | (u32::from(i64::from(result) >= sign) << 7)
        | (u32::from(!(-sign..sign).contains(&signed_result)) << 11);
    (result, flags)
}

#[test]
fn byte_subtraction_exhausts_all_values_and_incoming_borrows() {
    let mut image = load_pe32(&executable::pe32(&[0x1a, 0xc1]), 3).unwrap();
    for left in 0..256 {
        for right in 0..256 {
            for borrow in 0..2 {
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Eax, 0x1234_5600 | left);
                cpu.set_register(Register32::Ecx, right);
                cpu.eflags = 0xced6 | borrow;
                assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                let (result, flags) = oracle(left, right, borrow, 8);
                assert_eq!(cpu.register(Register32::Eax), 0x1234_5600 | result);
                assert_eq!(cpu.register(Register32::Ecx), right);
                assert_eq!(
                    cpu.eflags,
                    (0xced6 & !0x8d5) | flags,
                    "{left} {right} {borrow}"
                );
            }
        }
    }
}

fn forms(bits: u32, right: u32) -> Vec<(Vec<u8>, bool, u32)> {
    let wide = u8::from(bits != 8);
    let mut forms = Vec::new();
    for (mut code, immediate, memory_destination) in [
        (vec![0x1c + wide], true, false),
        (vec![0x80 + wide, 0xd8], true, false),
        (vec![0x80 + wide, 0x1d, 0, 0x20, 0x40, 0], true, true),
        (vec![0x18 + wide, 0xd8], false, false),
        (vec![0x1a + wide, 0xc3], false, false),
        (vec![0x18 + wide, 0x1d, 0, 0x20, 0x40, 0], false, true),
        (vec![0x1a + wide, 0x05, 4, 0x20, 0x40, 0], false, false),
    ] {
        if bits == 16 {
            code.insert(0, 0x66);
        }
        if immediate {
            code.extend_from_slice(&right.to_le_bytes()[..usize::try_from(bits / 8).unwrap()]);
        }
        forms.push((code, memory_destination, right));
    }
    if bits != 8 {
        let byte = right.to_le_bytes()[0];
        let actual = i32::from(byte.cast_signed()).cast_unsigned() & (u32::MAX >> (32 - bits));
        for (mut code, memory_destination) in [
            (vec![0x83, 0xd8], false),
            (vec![0x83, 0x1d, 0, 0x20, 0x40, 0], true),
        ] {
            if bits == 16 {
                code.insert(0, 0x66);
            }
            code.push(byte);
            forms.push((code, memory_destination, actual));
        }
    }
    forms
}

#[test]
fn all_subtract_encodings_and_widths_match_independent_boundary_oracle() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let sign = 1 << (bits - 1);
        for left in [0, 1, 15, 16, sign - 1, sign, mask] {
            for right in [0, 1, 15, 16, 0x80, sign - 1, sign, mask] {
                for borrow in 0..2 {
                    for (code, memory_destination, actual_right) in forms(bits, right) {
                        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                        let initial = (0x1234_5678 & !mask) | left;
                        image
                            .memory
                            .write(0x0040_2000, &initial.to_le_bytes())
                            .unwrap();
                        image
                            .memory
                            .write(0x0040_2004, &right.to_le_bytes())
                            .unwrap();
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_register(Register32::Eax, initial);
                        cpu.set_register(Register32::Ebx, right);
                        cpu.eflags = 0xced6 | borrow;
                        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1, "{code:02x?}");
                        let (result, flags) = oracle(left, actual_right, borrow, bits);
                        let expected = (initial & !mask) | result;
                        assert_eq!(
                            cpu.register(Register32::Eax),
                            if memory_destination {
                                initial
                            } else {
                                expected
                            }
                        );
                        let mut bytes = [0; 4];
                        image.memory.read(0x0040_2000, &mut bytes).unwrap();
                        assert_eq!(
                            u32::from_le_bytes(bytes),
                            if memory_destination {
                                expected
                            } else {
                                initial
                            }
                        );
                        assert_eq!(
                            cpu.eflags,
                            (0xced6 & !0x8d5) | flags,
                            "{bits} {left:x} {actual_right:x} {borrow} {code:02x?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn negation_handles_minimum_signed_values_narrow_registers_and_memory() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let sign = 1 << (bits - 1);
        let values: Vec<_> = if bits == 8 {
            (0..256).collect()
        } else {
            vec![0, 1, 15, 16, sign - 1, sign, mask]
        };
        let opcode = if bits == 8 { 0xf6 } else { 0xf7 };
        let mut forms = vec![
            (vec![opcode, 0xd8], false, 0),
            (vec![opcode, 0x1d, 0, 0x20, 0x40, 0], true, 0),
        ];
        if bits == 8 {
            forms.push((vec![opcode, 0xdc], false, 8));
        }
        for input in values {
            for (mut code, memory_destination, shift) in forms.clone() {
                if bits == 16 {
                    code.insert(0, 0x66);
                }
                let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                let initial = (0x1234_5678 & !(mask << shift)) | (input << shift);
                image
                    .memory
                    .write(0x0040_2000, &initial.to_le_bytes())
                    .unwrap();
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Eax, initial);
                cpu.eflags = 0xced7;
                assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                let (result, flags) = oracle(0, input, 0, bits);
                let expected = (initial & !(mask << shift)) | (result << shift);
                assert_eq!(
                    cpu.register(Register32::Eax),
                    if memory_destination {
                        initial
                    } else {
                        expected
                    }
                );
                let mut bytes = [0; 4];
                image.memory.read(0x0040_2000, &mut bytes).unwrap();
                assert_eq!(
                    u32::from_le_bytes(bytes),
                    if memory_destination {
                        expected
                    } else {
                        initial
                    }
                );
                assert_eq!(cpu.eflags, (0xced7 & !0x8d5) | flags);
            }
        }
    }
}

#[test]
fn borrow_writes_fault_atomically_and_unsupported_encodings_stay_closed() {
    for opcode in [[0xf7, 0x1d], [0x19, 0x1d]] {
        for address in [0x0040_2000_u32, 0x0040_2ffe, 0xffff_fffe] {
            let mut code = opcode.to_vec();
            code.extend_from_slice(&address.to_le_bytes());
            let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
            if address == 0xffff_fffe {
                image
                    .memory
                    .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
            }
            image.memory.write(u64::from(address), &[0xff; 2]).unwrap();
            if address == 0x0040_2000 {
                image
                    .memory
                    .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
                    .unwrap();
            }
            if address == 0x0040_2ffe {
                image
                    .memory
                    .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
                    .unwrap();
            }
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.eflags = 0xced7;
            let before = cpu;
            let result = cpu.run(&mut image.memory, 1);
            assert!(matches!(result.reason, StopReason::MemoryFault(_)));
            assert_eq!(result.instructions, 0);
            assert_eq!(cpu, before);
            let mut bytes = [0; 2];
            image.memory.read(u64::from(address), &mut bytes).unwrap();
            assert_eq!(bytes, [0xff; 2]);
        }
    }
    for code in [
        &[0xf0, 0xf7, 0x1d, 0, 0x20, 0x40, 0][..],
        &[0xf3, 0x1b, 0xc0],
        &[0x67, 0xf7, 0x18],
        &[0x82, 0xd8, 0],
        &[0x64, 0x67, 0x19, 0],
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
}

#[test]
fn multiword_borrow_high_bytes_and_fs_memory_resume_consistently() {
    let mut whole = load_pe32(&borrow_executable::pe32(), 3).unwrap();
    let mut stepped = load_pe32(&borrow_executable::pe32(), 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.eflags = 0xced7;
    let mut other = cpu;
    let result = cpu.run(&mut whole.memory, 100);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 12);
    let mut count = 0;
    for _ in 0..100 {
        let step = other.run(&mut stepped.memory, 1);
        count += step.instructions;
        if step.reason != StopReason::InstructionLimit {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(count, result.instructions);
    assert_eq!(other, cpu);
    assert_eq!(cpu.register(Register32::Eax), 1);
    assert_eq!(cpu.register(Register32::Edx), 0);
    assert_eq!(cpu.register(Register32::Ebx), 0xffff_fdff);
    assert_eq!(cpu.eflags, (0xced7 & !0x8d5) | 0x44);
    for memory in [&whole.memory, &stepped.memory] {
        let mut bytes = [0; 4];
        memory.read(0x0040_2000, &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), 16);
    }
}
