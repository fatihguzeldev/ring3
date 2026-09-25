use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

pub const INPUT: u32 = 0x0040_2200;
pub const OUTPUT: u32 = 0x0040_2300;

pub fn load(values: &[f64]) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for i in 0..values.len() {
        code.extend([0xdd, 0x05]);
        code.extend((INPUT + u32::try_from(i).unwrap() * 8).to_le_bytes());
    }
    code.extend([0xd9, 0xee, 0xdf, 0xe0]);
    for i in 0..=values.len() {
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

pub fn positive_zero_preserves_lower_stack_across_budgets() {
    let values = [-0.0, 1.25, -2.5, 3.0, f64::MIN_POSITIVE, -f64::MAX, 42.0];
    for depth in 0..=7 {
        for pc in [0, 0x100, 0x200, 0x300] {
            for rc in [0, 0x400, 0x800, 0xc00] {
                check(&values[..depth], pc | rc | 0x7f);
            }
        }
    }
}

fn check(values: &[f64], control: u16) {
    for budget in [1, 3, 30] {
        let (mut cpu, mut memory) = load(values);
        cpu.set_x87_control_word(control);
        let before = cpu;
        let pages = memory.mapped_pages();
        assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
        assert_eq!(cpu, before);
        let mut steps = 0;
        let prefix_steps = u64::try_from(values.len()).unwrap() + 2;
        while steps < prefix_steps {
            let run = cpu.run(&mut memory, budget.min(prefix_steps - steps));
            assert_eq!(run.reason, StopReason::InstructionLimit);
            assert!(run.instructions > 0);
            steps += run.instructions;
        }
        assert_eq!(cpu.x87_control_word(), control);
        // the existing store supports fewer profiles than an exact constant load.
        cpu.set_x87_control_word(0x027f);
        loop {
            let run = cpu.run(&mut memory, budget);
            steps += run.instructions;
            if run.reason == StopReason::Breakpoint {
                break;
            }
            assert_eq!(run.reason, StopReason::InstructionLimit);
            assert!(steps < 20);
        }
        assert_eq!(steps, u64::try_from(values.len() * 2 + 4).unwrap());
        assert_eq!(read(&memory, OUTPUT), 0);
        for (i, value) in values.iter().rev().enumerate() {
            assert_eq!(
                read(&memory, OUTPUT + u32::try_from(i + 1).unwrap() * 8),
                value.to_bits()
            );
        }
        assert_eq!(
            cpu.register(Register32::Eax),
            0xabcd_0000 | ((7 - u32::try_from(values.len()).unwrap()) << 11)
        );
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
        assert_eq!(cpu.eflags, before.eflags);
        assert_eq!(cpu.fs_base(), before.fs_base());
        assert_eq!(cpu.x87_control_word(), 0x027f);
        assert_eq!(memory.mapped_pages(), pages);
        for (i, value) in values.iter().enumerate() {
            assert_eq!(
                read(&memory, INPUT + u32::try_from(i).unwrap() * 8),
                value.to_bits()
            );
        }
    }
}
