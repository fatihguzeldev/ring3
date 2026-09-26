use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2300;

fn load(values: &[f64], index: u8, control: u16) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for position in 0..values.len() {
        code.extend([0xdd, 0x05]);
        code.extend((INPUT + u32::try_from(position).unwrap() * 8).to_le_bytes());
    }
    code.extend([0xd8, 0xf8 + index, 0xdf, 0xe0]);
    for position in 0..values.len() {
        code.extend([0xdd, 0x1d]);
        code.extend((OUTPUT + u32::try_from(position).unwrap() * 8).to_le_bytes());
    }
    code.push(0xcc);
    let mut image = load_pe32(&super::executable::pe32(&code), 16).unwrap();
    for (position, value) in values.iter().enumerate() {
        image
            .memory
            .write(
                u64::from(INPUT) + u64::try_from(position).unwrap() * 8,
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    cpu.set_fs_base(0xdead_0000);
    cpu.set_register(Register32::Ebx, 0xabcd_1234);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn register_reverse_divide() {
    for (values, index, control, expected, status) in [
        (vec![2.0], 0, 0x027f, 1.0, 0),
        (vec![6.0, 2.0], 1, 0x027f, 3.0, 0),
        (vec![12.0, 7.0, 3.0], 2, 0x027f, 4.0, 0),
        (vec![0.0, -2.0], 1, 0x027f, -0.0, 0),
        (vec![1.0, 3.0], 1, 0x007f, f64::from(1.0_f32 / 3.0), 0x220),
    ] {
        for budget in [1, 3, 30] {
            let (mut cpu, mut memory) = load(&values, index, control);
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
            for (position, value) in values[..values.len() - 1].iter().rev().enumerate() {
                assert_eq!(
                    read(&memory, OUTPUT + u32::try_from(position + 1).unwrap() * 8),
                    value.to_bits()
                );
            }
            let stack_top = (8 - u32::try_from(values.len()).unwrap()) << 11;
            assert_eq!(cpu.register(Register32::Eax), stack_top | status);
            assert_eq!(
                cpu.register(Register32::Ebx),
                before.register(Register32::Ebx)
            );
            assert_eq!(cpu.eflags, before.eflags);
            assert_eq!(cpu.fs_base(), before.fs_base());
            assert_eq!(cpu.x87_control_word(), before.x87_control_word());
        }
    }
}

pub fn register_reverse_divide_rejects_atomically() {
    for (values, index, control) in [
        (vec![1.0, 0.0], 1, 0x027f),
        (vec![f64::MAX, 0.5], 1, 0x027f),
        (vec![f64::MIN_POSITIVE, 2.0], 1, 0x027f),
        (vec![1.0, 3.0], 1, 0x027e),
        (vec![1.0, 3.0], 1, 0x0c7f),
        (vec![3.0], 1, 0x027f),
    ] {
        let (mut cpu, mut memory) = load(&values, index, 0x027f);
        assert_eq!(
            cpu.run(&mut memory, u64::try_from(values.len()).unwrap())
                .instructions,
            u64::try_from(values.len()).unwrap()
        );
        cpu.set_x87_control_word(control);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(read(&memory, OUTPUT), 0);
    }
}
