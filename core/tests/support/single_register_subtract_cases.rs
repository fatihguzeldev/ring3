use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

pub const INPUT: u32 = 0x0040_2200;
pub const OUTPUT: u32 = 0x0040_2300;

pub fn load(values: &[f64], index: u8) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for i in 0..values.len() {
        code.extend([0xdd, 0x05]);
        code.extend((INPUT + u32::try_from(i).unwrap() * 8).to_le_bytes());
    }
    code.extend([0xd8, 0xe0 + index, 0xdf, 0xe0]);
    for i in 0..values.len() {
        code.extend([0xdd, 0x1d]);
        code.extend((OUTPUT + u32::try_from(i).unwrap() * 8).to_le_bytes());
    }
    code.push(0xcc);
    let mut image = load_pe32(&super::executable::pe32(&code), 16).unwrap();
    for (i, value) in values.iter().enumerate() {
        image
            .memory
            .write(
                u64::from(INPUT) + u64::try_from(i).unwrap() * 8,
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    cpu.set_fs_base(0xdead_0000);
    for register in [
        Register32::Eax,
        Register32::Ebx,
        Register32::Ecx,
        Register32::Edx,
        Register32::Esi,
        Register32::Edi,
        Register32::Ebp,
        Register32::Esp,
    ] {
        cpu.set_register(register, 0xabcd_1234);
    }
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

pub fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn arithmetic() {
    let half_ulp = 2.0_f64.powi(-24);
    let next = f64::from(f32::from_bits(0x3f80_0001));
    for (top, source, expected, status) in [
        (0.0, 0.0, 0.0, 0),
        (5.0, 2.0, 3.0, 0),
        (2.0, 5.0, -3.0, 0),
        (-0.0, 0.0, -0.0, 0),
        (-0.0, -0.0, 0.0, 0),
        (1.0, -half_ulp, 1.0, 0x20),
        (-1.0, half_ulp, -1.0, 0x20),
        (
            next,
            -half_ulp,
            f64::from(f32::from_bits(0x3f80_0002)),
            0x220,
        ),
        (
            -next,
            half_ulp,
            f64::from(f32::from_bits(0xbf80_0002)),
            0x220,
        ),
        (1.0, f64::MIN_POSITIVE, 1.0, 0x220),
        (1.0, -f64::MIN_POSITIVE, 1.0, 0x20),
        (1.0 + half_ulp, 2.0_f64.powi(-80), 1.0, 0x20),
        (1.0 + half_ulp, -2.0_f64.powi(-80), next, 0x220),
        (
            f64::from(f32::MIN_POSITIVE),
            0.0,
            f64::from(f32::MIN_POSITIVE),
            0,
        ),
        (f64::from(f32::MAX), 0.0, f64::from(f32::MAX), 0),
    ] {
        for index in 1..=7_u8 {
            let mut values = vec![source];
            values.extend(vec![3.5; usize::from(index) - 1]);
            values.push(top);
            check(&values, index, expected, status);
        }
    }
}

fn check(values: &[f64], index: u8, expected: f64, status: u32) {
    for budget in [1, 4, 30] {
        let (mut cpu, mut memory) = load(values, index);
        let before = cpu;
        assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
        assert_eq!(cpu, before);
        let mut steps = 0;
        loop {
            let run = cpu.run(&mut memory, budget);
            steps += run.instructions;
            if run.reason == StopReason::Breakpoint {
                break;
            }
            assert_eq!(run.reason, StopReason::InstructionLimit);
            assert!(steps < 30);
        }
        assert_eq!(steps, u64::try_from(values.len() * 2 + 3).unwrap());
        assert_eq!(read(&memory, OUTPUT), expected.to_bits());
        for (i, value) in values[..values.len() - 1].iter().rev().enumerate() {
            assert_eq!(
                read(&memory, OUTPUT + u32::try_from(i + 1).unwrap() * 8),
                value.to_bits()
            );
        }
        let stack_top = (8 - u32::try_from(values.len()).unwrap()) << 11;
        assert_eq!(
            cpu.register(Register32::Eax),
            0xabcd_0000 | stack_top | status
        );
        assert_eq!(cpu.eflags, before.eflags);
        assert_eq!(cpu.fs_base(), before.fs_base());
        assert_eq!(cpu.x87_control_word(), before.x87_control_word());
        for register in [
            Register32::Ebx,
            Register32::Ecx,
            Register32::Edx,
            Register32::Esi,
            Register32::Edi,
            Register32::Ebp,
            Register32::Esp,
        ] {
            assert_eq!(cpu.register(register), before.register(register));
        }
    }
}
