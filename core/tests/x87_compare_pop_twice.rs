#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = INPUT + 0x100;

fn load(values: &[f64]) -> (Cpu32, GuestMemory) {
    let mut code = Vec::new();
    for i in 0..values.len() {
        code.extend([0xdd, 0x05]);
        code.extend((INPUT + u32::try_from(i).unwrap() * 8).to_le_bytes());
    }
    code.extend([0xde, 0xd9, 0xdf, 0xe0]);
    for i in 0..values.len().saturating_sub(2) {
        code.extend([0xdd, 0x1d]);
        code.extend((OUTPUT + u32::try_from(i).unwrap() * 8).to_le_bytes());
    }
    code.push(0xcc);
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    for (i, value) in values.iter().enumerate() {
        image
            .memory
            .write(
                u64::from(INPUT) + u64::try_from(i).unwrap() * 8,
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    image.memory.write(u64::from(OUTPUT), &[0x55; 64]).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.set_register(Register32::Eax, 0xabcd_1234);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn comparison_pops_exactly_two_and_preserves_lower_values_across_profiles_and_budgets() {
    for (left, right, condition) in [
        (-3.0_f64, -0.0_f64, 0x100),
        (3.0, 1.0, 0),
        (4.0, 4.0, 0x4000),
        (-0.0, 0.0, 0x4000),
    ] {
        for depth in [2, 3, 8] {
            let mut values = [-0.0, 1.25, -2.5, 7.0, -11.0, 0.5, 9.0, 13.0][..depth].to_vec();
            values[depth - 2] = right;
            values[depth - 1] = left;
            for profile in 0..16 {
                for budget in [1, 3, 40] {
                    let (mut cpu, mut memory) = load(&values);
                    cpu.set_x87_control_word(0x007f | (profile << 8));
                    let prefix = u64::try_from(depth).unwrap() + 2;
                    let mut steps = 0;
                    while steps < prefix {
                        let run = cpu.run(&mut memory, budget.min(prefix - steps));
                        assert_eq!(run.reason, StopReason::InstructionLimit);
                        assert!(run.instructions > 0);
                        steps += run.instructions;
                    }
                    assert_eq!(
                        cpu.register(Register32::Eax),
                        0xabcd_0000
                            | (((10 - u32::try_from(depth).unwrap()) & 7) << 11)
                            | condition
                    );
                    assert_eq!(cpu.x87_control_word(), 0x007f | (profile << 8));
                    cpu.set_x87_control_word(0x027f);
                    assert_eq!(cpu.run(&mut memory, 20).reason, StopReason::Breakpoint);
                    for (i, value) in values[..depth - 2].iter().rev().enumerate() {
                        assert_eq!(
                            read(&memory, OUTPUT + u32::try_from(i).unwrap() * 8),
                            value.to_bits()
                        );
                    }
                    assert_eq!(
                        read(&memory, OUTPUT + u32::try_from(depth - 2).unwrap() * 8),
                        0x5555_5555_5555_5555
                    );
                    assert_eq!(cpu.eflags, 0xced7);
                }
            }
        }
    }
}

fn refuses(cpu: &mut Cpu32, memory: &mut GuestMemory) {
    let before = *cpu;
    let output = read(memory, OUTPUT);
    let run = cpu.run(memory, 1);
    assert_eq!(run.reason, StopReason::UnsupportedInstruction);
    assert_eq!(run.instructions, 0);
    assert_eq!(*cpu, before);
    assert_eq!(read(memory, OUTPUT), output);
}

#[test]
fn fewer_than_two_values_refuse_without_popping() {
    for values in [&[][..], &[1.0][..]] {
        let (mut cpu, mut memory) = load(values);
        let depth = u64::try_from(values.len()).unwrap();
        assert_eq!(cpu.run(&mut memory, depth).instructions, depth);
        refuses(&mut cpu, &mut memory);
    }
}

#[test]
fn unsupported_operand_on_either_side_refuses_without_popping() {
    for bits in [
        0x7ff8_0000_0000_0011,
        0x7ff0_0000_0000_0001,
        1,
        0x8000_0000_0000_0001,
    ] {
        for side in 0..2 {
            let mut values = [1.0; 2];
            values[side] = f64::from_bits(bits);
            let (mut cpu, mut memory) = load(&values);
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            refuses(&mut cpu, &mut memory);
        }
    }
}

#[test]
fn unmasked_exceptions_refuse_and_mask_repair_retries() {
    for bit in 0..6 {
        let (mut cpu, mut memory) = load(&[-0.0, -3.0]);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        cpu.set_x87_control_word(0x027f & !(1 << bit));
        refuses(&mut cpu, &mut memory);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_0100);
    }
}

#[test]
fn register_comparison_reads_no_data_memory() {
    let (mut cpu, mut memory) = load(&[2.0, 1.0]);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    let registers = [
        Register32::Ebx,
        Register32::Ecx,
        Register32::Edx,
        Register32::Esi,
        Register32::Edi,
        Register32::Ebp,
        Register32::Esp,
    ];
    for register in registers {
        cpu.set_register(register, 0xdead_beef);
    }
    let pages = memory.mapped_pages();
    memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_0100);
    assert_eq!(cpu.eflags, 0xced7);
    assert_eq!(memory.mapped_pages(), pages);
    for register in registers {
        assert_eq!(cpu.register(register), 0xdead_beef);
    }
}
