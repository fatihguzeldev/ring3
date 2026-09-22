use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

use super::{executable, unsigned_division_executable::encoding};

fn dividend(cpu: &mut Cpu32, bits: u32, value: u128) {
    let mask = (1_u128 << bits) - 1;
    let low_mask = if bits == 8 { 0xffff } else { mask };
    cpu.set_register(
        Register32::Eax,
        (0xabcd_1234 & !u32::try_from(low_mask).unwrap())
            | u32::try_from(value & low_mask).unwrap(),
    );
    cpu.set_register(
        Register32::Edx,
        if bits == 8 {
            0xface_5678
        } else {
            (0xface_5678 & !u32::try_from(mask).unwrap())
                | u32::try_from((value >> bits) & mask).unwrap()
        },
    );
}

pub fn arithmetic() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        for divisor in [1, 2, 3, 17, mask] {
            for quotient in [0, 1, mask / 2, mask] {
                for remainder in [0, divisor - 1] {
                    for in_memory in [false, true] {
                        let code = encoding(
                            bits,
                            if in_memory { 5 } else { 0xc3 },
                            in_memory.then_some(0x0040_2ffc),
                        );
                        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
                        image
                            .memory
                            .write(0x0040_2ffc, &divisor.to_le_bytes())
                            .unwrap();
                        image
                            .memory
                            .protect(0x0040_2000, 4096, Permissions::READ)
                            .unwrap();
                        let mut cpu = Cpu32::new(image.entry_point);
                        dividend(
                            &mut cpu,
                            bits,
                            u128::from(quotient) * u128::from(divisor) + u128::from(remainder),
                        );
                        cpu.set_register(Register32::Ebx, (0xdada_1234 & !mask) | divisor);
                        cpu.eflags = 0xced7;
                        let mut expected = cpu;
                        expected.eip += u32::try_from(code.len()).unwrap();
                        if bits == 8 {
                            expected.set_register(
                                Register32::Eax,
                                (cpu.register(Register32::Eax) & !0xffff)
                                    | (remainder << 8)
                                    | quotient,
                            );
                        } else {
                            expected.set_register(
                                Register32::Eax,
                                (cpu.register(Register32::Eax) & !mask) | quotient,
                            );
                            expected.set_register(
                                Register32::Edx,
                                (cpu.register(Register32::Edx) & !mask) | remainder,
                            );
                        }
                        let run = cpu.run(&mut image.memory, 1);
                        assert_eq!(run.reason, StopReason::InstructionLimit);
                        assert_eq!(run.instructions, 1);
                        assert_eq!(
                            cpu, expected,
                            "{bits}/{divisor}/{quotient}/{remainder}/{in_memory}"
                        );
                        let mut after = [0; 4];
                        image.memory.read(0x0040_2ffc, &mut after).unwrap();
                        assert_eq!(after, divisor.to_le_bytes());
                    }
                }
            }
        }
    }
}

pub fn errors() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        for (divisor, numerator) in [
            (0, 0),
            (0, u128::from(mask)),
            (1, u128::from(mask) + 1),
            (mask, (u128::from(mask) + 1) * u128::from(mask)),
        ] {
            for in_memory in [false, true] {
                let code = encoding(
                    bits,
                    if in_memory { 5 } else { 0xc3 },
                    in_memory.then_some(0x0040_2ffc),
                );
                let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
                image
                    .memory
                    .write(0x0040_2ffc, &divisor.to_le_bytes())
                    .unwrap();
                let mut cpu = Cpu32::new(image.entry_point);
                dividend(&mut cpu, bits, numerator);
                cpu.set_register(Register32::Ebx, divisor);
                cpu.eflags = 0xced7;
                let before = cpu;
                assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
                assert_eq!(cpu, before);
                for _ in 0..2 {
                    let run = cpu.run(&mut image.memory, 1);
                    assert_eq!(run.reason, StopReason::DivideError);
                    assert_eq!(run.instructions, 0);
                    assert_eq!(run.instruction_pointer, before.eip);
                    assert_eq!(cpu, before);
                }
            }
        }
    }
}
