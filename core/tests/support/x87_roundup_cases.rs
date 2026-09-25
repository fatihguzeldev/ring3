use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

fn check(
    opcode: &[u8],
    source: f64,
    top: f64,
    control: u16,
    expected: u64,
    status: u32,
    pop: bool,
) {
    let mut code = vec![
        0xdd, 0x05, 0x00, 0x22, 0x40, 0, 0xdd, 0x05, 0x08, 0x22, 0x40, 0,
    ];
    code.extend(opcode);
    code.extend([0xdf, 0xe0, 0xdd, 0x1d, 0x20, 0x22, 0x40, 0, 0xcc]);
    for budget in [1, 20] {
        let mut image = load_pe32(&super::executable::pe32(&code), 16).unwrap();
        image
            .memory
            .write(0x0040_2200, &source.to_le_bytes())
            .unwrap();
        image.memory.write(0x0040_2208, &top.to_le_bytes()).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_x87_control_word(control);
        cpu.set_register(Register32::Eax, 0xabcd_1234);
        cpu.eflags = 0xced7;
        let mut steps = 0;
        loop {
            let run = cpu.run(&mut image.memory, budget);
            steps += run.instructions;
            if run.reason == StopReason::Breakpoint {
                break;
            }
            assert_eq!(run.reason, StopReason::InstructionLimit);
            assert!(steps < 6);
        }
        assert_eq!(steps, 6);
        let mut result = [0; 8];
        image.memory.read(0x0040_2220, &mut result).unwrap();
        assert_eq!(u64::from_le_bytes(result), expected);
        assert_eq!(
            cpu.register(Register32::Eax),
            0xabcd_0000 | if pop { 0x3800 } else { 0x3000 } | status,
            "opcode={opcode:x?} source={source} top={top} cw={control:x}"
        );
        assert_eq!(cpu.eflags, 0xced7);
        assert_eq!(cpu.x87_control_word(), control);
    }
}

pub fn sums() {
    for (control, half, next, next_two) in [
        (
            0x007f,
            2.0_f64.powi(-24),
            f64::from(f32::from_bits(0x3f80_0001)),
            f64::from(f32::from_bits(0x3f80_0002)),
        ),
        (
            0x027f,
            2.0_f64.powi(-53),
            f64::from_bits(0x3ff0_0000_0000_0001),
            f64::from_bits(0x3ff0_0000_0000_0002),
        ),
    ] {
        for (base, result, status) in [(1.0, 1.0_f64, 0x20), (next, next_two, 0x220)] {
            for sign in [1.0, -1.0] {
                for (opcode, source, top, pop) in [
                    (&[0xd8, 0xc1][..], half, base, false),
                    (&[0xde, 0xc1][..], half, base, true),
                    (&[0xd8, 0xe9][..], base, -half, false),
                    (&[0xde, 0xe1][..], -half, base, true),
                    (&[0xde, 0xe9][..], base, -half, true),
                ] {
                    check(
                        opcode,
                        source * sign,
                        top * sign,
                        control,
                        (result * sign).to_bits(),
                        status,
                        pop,
                    );
                }
                if control == 0x027f {
                    check(
                        &[0xd8, 0xe1],
                        -half * sign,
                        base * sign,
                        control,
                        (result * sign).to_bits(),
                        status,
                        false,
                    );
                    check(
                        &[0xdc, 0x05, 0, 0x22, 0x40, 0],
                        half * sign,
                        base * sign,
                        control,
                        (result * sign).to_bits(),
                        status,
                        false,
                    );
                    check(
                        &[0xdc, 0x25, 0, 0x22, 0x40, 0],
                        -half * sign,
                        base * sign,
                        control,
                        (result * sign).to_bits(),
                        status,
                        false,
                    );
                }
            }
        }
    }
}

pub fn products_and_quotients() {
    for (control, expected, status) in [
        (0x007f, f64::from(f32::from_bits(0x3eaa_aaab)), 0x220),
        (0x027f, f64::from_bits(0x3fd5_5555_5555_5555), 0x20),
    ] {
        for sign in [1.0, -1.0] {
            for (opcode, source, top, pop) in [
                (&[0xd8, 0xf1][..], 3.0, sign, false),
                (&[0xde, 0xf9][..], sign, 3.0, true),
            ] {
                check(
                    opcode,
                    source,
                    top,
                    control,
                    (expected * sign).to_bits(),
                    status,
                    pop,
                );
            }
        }
    }
    for (input, expected, status) in [
        (1.0 + 2.0_f64.powi(-24), 1.0_f64, 0x20),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            f64::from(f32::from_bits(0x3f80_0002)),
            0x220,
        ),
    ] {
        for sign in [1.0, -1.0] {
            for (opcode, pop) in [(&[0xd8, 0xc9][..], false), (&[0xde, 0xc9][..], true)] {
                check(
                    opcode,
                    input * sign,
                    1.0,
                    0x007f,
                    (expected * sign).to_bits(),
                    status,
                    pop,
                );
            }
        }
    }
}
