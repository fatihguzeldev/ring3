use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

pub fn set_dividend(cpu: &mut Cpu32, value: i64) {
    let bytes = value.to_le_bytes();
    cpu.set_register(
        Register32::Eax,
        u32::from_le_bytes(bytes[..4].try_into().unwrap()),
    );
    cpu.set_register(
        Register32::Edx,
        u32::from_le_bytes(bytes[4..].try_into().unwrap()),
    );
}

pub fn load(code: &[u8], dividend: i64, divisor: i32) -> (Cpu32, GuestMemory) {
    let mut program = vec![0xdd, 0x05, 0x20, 0x20, 0x40, 0];
    program.extend(code);
    let mut image = load_pe32(&super::executable::pe32(&program), 16).unwrap();
    image
        .memory
        .write(0x0040_2020, &3.5_f64.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(0x0040_2ffc, &divisor.to_le_bytes())
        .unwrap();
    image
        .memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    cpu.set_x87_control_word(0x0b40);
    set_dividend(&mut cpu, dividend);
    for (register, value) in [
        (Register32::Ebx, 0x1122_3344),
        (Register32::Ecx, 0xaabb_ccdd),
        (Register32::Esi, divisor.cast_unsigned()),
        (Register32::Edi, 0x5555_5555),
        (Register32::Ebp, 0x9876_5432),
        (Register32::Esp, 0x7654_3210),
    ] {
        cpu.set_register(register, value);
    }
    cpu.set_fs_base(0x1234_0000);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn check(dividend: i64, divisor: i32, quotient: i32, remainder: i32) {
    for code in [&[0xf7, 0xfe][..], &[0xf7, 0x3d, 0xfc, 0x2f, 0x40, 0][..]] {
        for budget in [1, 3] {
            let mut program = code.to_vec();
            program.extend([0x90, 0xcc]);
            let (mut cpu, mut memory) = load(&program, dividend, divisor);
            let mut expected = cpu;
            assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
            assert_eq!(cpu, expected);
            expected.eip += u32::try_from(program.len()).unwrap();
            expected.set_register(Register32::Eax, quotient.cast_unsigned());
            expected.set_register(Register32::Edx, remainder.cast_unsigned());
            let mut steps = 0;
            loop {
                let run = cpu.run(&mut memory, budget);
                steps += run.instructions;
                if run.reason == StopReason::Breakpoint {
                    break;
                }
                assert_eq!(run.reason, StopReason::InstructionLimit);
                assert!(steps < 3);
            }
            assert_eq!(steps, 3);
            assert_eq!(cpu, expected, "{dividend}/{divisor}");
            let mut after = [0; 4];
            memory.read(0x0040_2ffc, &mut after).unwrap();
            assert_eq!(after, divisor.to_le_bytes());
        }
    }
}

pub fn arithmetic() {
    for (dividend, divisor, quotient, remainder) in [
        (71, 8, 8, 7),
        (-71, 8, -8, -7),
        (71, -8, -8, 7),
        (-71, -8, 8, -7),
        (4_294_967_296, 3, 1_431_655_765, 1),
        (4_294_967_297, i32::MAX, 2, 3),
        (-8_589_934_592, i32::MIN, 4, 0),
        (2_147_483_648, -1, i32::MIN, 0),
    ] {
        check(dividend, divisor, quotient, remainder);
    }
    for divisor in [i32::MIN, -17, -1, 1, 17, i32::MAX] {
        for quotient in [i32::MIN, -8, -1, 0, 1, 8, i32::MAX] {
            let magnitude = i128::from(divisor).abs() - 1;
            for remainder in [-magnitude, 0, magnitude] {
                let dividend = i128::from(quotient) * i128::from(divisor) + remainder;
                if remainder != 0 && remainder.signum() != dividend.signum() {
                    continue;
                }
                check(
                    i64::try_from(dividend).unwrap(),
                    divisor,
                    quotient,
                    i32::try_from(remainder).unwrap(),
                );
            }
        }
    }
}

pub fn errors() {
    for (dividend, divisor) in [
        (0, 0),
        (71, 0),
        (i64::MIN, -1),
        (i64::MIN, 1),
        (i64::MAX, -1),
        (i64::MAX, 1),
        (2_147_483_648, 1),
        (-2_147_483_649, 1),
        (i64::from(i32::MIN), -1),
        (i64::MIN, i32::MIN),
    ] {
        for code in [&[0xf7, 0xfe][..], &[0xf7, 0x3d, 0xfc, 0x2f, 0x40, 0][..]] {
            let (mut cpu, mut memory) = load(code, dividend, divisor);
            let before = cpu;
            for _ in 0..2 {
                let run = cpu.run(&mut memory, 1);
                assert_eq!(run.reason, StopReason::DivideError);
                assert_eq!((run.instructions, run.instruction_pointer), (0, before.eip));
                assert_eq!(cpu, before);
            }
        }
    }
}
