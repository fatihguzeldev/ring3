#[path = "support/executable.rs"]
mod executable;
#[path = "support/x87_scaling_executable.rs"]
mod x87_scaling_executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, StopReason, load_pe32};

#[test]
fn authored_integer_scaling_agrees_whole_stepwise_and_with_known_bits() {
    for (input, factor, expected) in [
        (-7_i64, 0x3fc0_0000_u32, 0xc025_0000_0000_0000_u64),
        (3, 0x3eaa_aaab, 0x3ff0_0000_0800_0000),
        (0, 0xbf80_0000, 0x8000_0000_0000_0000),
        (1_i64 << 53, 0x3fc0_0000, 0x4348_0000_0000_0000),
        ((1_i64 << 52) + 1, 0x3fc0_0000, 0x4338_0000_0000_0002),
        ((1_i64 << 52) + 3, 0x3fc0_0000, 0x4338_0000_0000_0004),
    ] {
        for budget in [1, 20] {
            let mut image = load_pe32(&x87_scaling_executable::pe32(), 16).unwrap();
            image
                .memory
                .write(0x0040_2180, &input.to_le_bytes())
                .unwrap();
            image
                .memory
                .write(0x0040_2188, &factor.to_le_bytes())
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            let mut count = 0;
            loop {
                let run = cpu.run(&mut image.memory, budget);
                count += run.instructions;
                if run.reason != StopReason::InstructionLimit {
                    assert_eq!(run.reason, StopReason::Breakpoint);
                    break;
                }
                assert!(count < 20);
            }
            assert_eq!(count, 5);
            let mut bytes = [0; 8];
            image.memory.read(0x0040_21a0, &mut bytes).unwrap();
            assert_eq!(u64::from_le_bytes(bytes), expected);
        }
    }
}

fn operand(opcode: u8, mode: u8, address: u32) -> Vec<u8> {
    let mut code = vec![opcode, mode];
    code.extend_from_slice(&address.to_le_bytes());
    code
}

fn load(code: &[u8]) -> (Cpu32, GuestMemory) {
    let image = load_pe32(&executable::pe32(code), 32).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(0x0040_2300, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn signed_integer_widths_load_exact_values_without_rounding() {
    for (opcode, mode, size) in [(0xdf, 0x05, 2), (0xdb, 0x05, 4), (0xdf, 0x2d, 8)] {
        let mut code = operand(opcode, mode, 0x0040_2200);
        code.extend(operand(0xdd, 0x1d, 0x0040_2300));
        for (integer, expected) in [
            (0_i64, 0_u64),
            (1, 0x3ff0_0000_0000_0000),
            (-1, 0xbff0_0000_0000_0000),
            (-32768, 0xc0e0_0000_0000_0000),
            (32767, 0x40df_ffc0_0000_0000),
            (-2_147_483_648, 0xc1e0_0000_0000_0000),
            (2_147_483_647, 0x41df_ffff_ffc0_0000),
            (1_i64 << 53, 0x4340_0000_0000_0000),
            (-(1_i64 << 53), 0xc340_0000_0000_0000),
        ] {
            if (size == 2 && i16::try_from(integer).is_err())
                || (size == 4 && i32::try_from(integer).is_err())
            {
                continue;
            }
            let (mut cpu, mut memory) = load(&code);
            memory
                .write(0x0040_2200, &integer.to_le_bytes()[..size])
                .unwrap();
            let mut expected_cpu = cpu;
            expected_cpu.eip += 12;
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            assert_eq!(result(&memory), expected);
            assert_eq!(cpu, expected_cpu);
        }
    }
}

#[test]
fn integer_ranges_profiles_and_stack_capacity_fail_without_mutation() {
    let code = operand(0xdf, 0x2d, 0x0040_2200);
    for integer in [i64::MIN, i64::MAX, (1_i64 << 53) + 1, -(1_i64 << 53) - 1] {
        let (mut cpu, mut memory) = load(&code);
        memory.write(0x0040_2200, &integer.to_le_bytes()).unwrap();
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let (mut cpu, mut memory) = load(&code);
    cpu.set_x87_control_word(0x027e);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    cpu.set_x87_control_word(0x027f);
    let entry = cpu.eip;
    for _ in 0..8 {
        cpu.eip = entry;
        let before = cpu;
        assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
        assert_eq!(cpu, before);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    }
    cpu.eip = entry;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn integer_sources_check_full_width_and_fs_before_pushing() {
    for (opcode, mode, size) in [(0xdf, 0x05, 2_usize), (0xdb, 0x05, 4), (0xdf, 0x2d, 8)] {
        let address = u32::MAX - u32::try_from(size).unwrap() + 1;
        let mut code = operand(opcode, mode, address);
        code.extend(operand(0xdd, 0x1d, 0x0040_2300));
        let (mut cpu, mut memory) = load(&code);
        memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory
            .write(u64::from(address), &(-1_i64).to_le_bytes()[..size])
            .unwrap();
        memory
            .protect(0xffff_f000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(result(&memory), 0xbff0_0000_0000_0000);
        let (mut cpu, mut memory) = load(&operand(opcode, mode, address + 1));
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
    }
    let mut code = vec![0x64];
    code.extend(operand(0xdf, 0x2d, 0x0000_0ffc));
    code.extend(operand(0xdd, 0x1d, 0x0040_2300));
    let (mut cpu, mut memory) = load(&code);
    cpu.set_fs_base(0x0040_2000);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x0040_2ffc, &(-7_i64).to_le_bytes()).unwrap();
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(result(&memory), 0xc01c_0000_0000_0000);
}

fn multiplication(opcode: u8, top: u64, source: u64) -> (Cpu32, GuestMemory) {
    let mut code = operand(0xdd, 0x05, 0x0040_2200);
    code.extend(operand(opcode, 0x0d, 0x0040_2210));
    code.extend(operand(0xdd, 0x1d, 0x0040_2300));
    let (cpu, mut memory) = load(&code);
    memory.write(0x0040_2200, &top.to_le_bytes()).unwrap();
    memory.write(0x0040_2210, &source.to_le_bytes()).unwrap();
    (cpu, memory)
}

#[test]
fn multiplication_rounds_known_ties_and_preserves_zero_sign() {
    for opcode in [0xd8, 0xdc] {
        for (top, source32, source64, expected) in [
            (
                0x3ff0_0000_0000_0001_u64,
                0x3fc0_0000_u32,
                0x3ff8_0000_0000_0000_u64,
                0x3ff8_0000_0000_0002_u64,
            ),
            (
                0x3ff0_0000_0000_0003,
                0x3fc0_0000,
                0x3ff8_0000_0000_0000,
                0x3ff8_0000_0000_0004,
            ),
            (
                0x4010_0000_0000_0000,
                0xc000_0000,
                0xc000_0000_0000_0000,
                0xc020_0000_0000_0000,
            ),
            (0, 0xbf80_0000, 0xbff0_0000_0000_0000, 0x8000_0000_0000_0000),
            (0x8000_0000_0000_0000, 0xbf80_0000, 0xbff0_0000_0000_0000, 0),
            (0xc000_0000_0000_0000, 0, 0, 0x8000_0000_0000_0000),
            (0xc000_0000_0000_0000, 0x8000_0000, 0x8000_0000_0000_0000, 0),
        ] {
            let source = if opcode == 0xd8 {
                u64::from(source32)
            } else {
                source64
            };
            let (mut cpu, mut memory) = multiplication(opcode, top, source);
            let mut expected_cpu = cpu;
            expected_cpu.eip += 18;
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(result(&memory), expected);
            assert_eq!(cpu, expected_cpu);
        }
    }
}

#[test]
fn multiplication_exclusions_preserve_the_loaded_operand() {
    for (top, source) in [
        (f64::MAX, 2.0_f64),
        (f64::MIN_POSITIVE, 0.5),
        (f64::MIN_POSITIVE, f64::MIN_POSITIVE),
        (f64::MIN_POSITIVE, 1.0),
        ((2.0 * f64::MIN_POSITIVE).next_down(), 1.0),
        (1.0, f64::INFINITY),
        (1.0, f64::NAN),
        (1.0, f64::from_bits(1)),
    ] {
        let (mut cpu, mut memory) = multiplication(0xdc, top.to_bits(), source.to_bits());
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
    }
    let (mut cpu, mut memory) = multiplication(0xdc, f64::MIN_POSITIVE.to_bits(), 2_f64.to_bits());
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(result(&memory), 0x0020_0000_0000_0000);
}
