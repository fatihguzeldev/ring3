#[path = "support/executable.rs"]
mod executable;
#[path = "support/x87_executable.rs"]
mod x87_executable;
use ring3_core::execution::{Cpu32, GuestMemory, StopReason, load_pe32};

#[test]
fn authored_load_compute_store_sequence_runs_whole_or_stepwise() {
    for (input, dividend, expected) in [
        (0x4080_0000_u32, 0x4100_0000_u32, 0x4080_0000_u32),
        (0x4110_0000, 0x3f80_0000, 0x3eaa_aaab),
        (0x4000_0000, 0x3f80_0000, 0x3f35_04f3),
    ] {
        for budget in [1, 20] {
            let mut image = load_pe32(&x87_executable::pe32(), 16).unwrap();
            image
                .memory
                .write(0x0040_2180, &input.to_le_bytes())
                .unwrap();
            image
                .memory
                .write(0x0040_2184, &dividend.to_le_bytes())
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            let mut steps = 0;
            loop {
                let run = cpu.run(&mut image.memory, budget);
                steps += run.instructions;
                if run.reason != StopReason::InstructionLimit {
                    assert_eq!(run.reason, StopReason::Breakpoint);
                    break;
                }
                assert!(steps < 20);
            }
            assert_eq!(steps, 6);
            let mut bytes = [0; 4];
            image.memory.read(0x0040_2188, &mut bytes).unwrap();
            assert_eq!(u32::from_le_bytes(bytes), expected);
        }
    }
}

fn load(operation: &[u8], top: u64, source: u64) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05, 0, 0x22, 0x40, 0];
    code.extend_from_slice(operation);
    code.extend_from_slice(&[0xdd, 0x1d, 0x20, 0x22, 0x40, 0]);
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image.memory.write(0x0040_2200, &top.to_le_bytes()).unwrap();
    image
        .memory
        .write(0x0040_2210, &source.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(0x0040_2220, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn square_root_matches_known_bits_and_preserves_signed_zero() {
    for (input, expected) in [
        (0_u64, 0_u64),
        (0x8000_0000_0000_0000, 0x8000_0000_0000_0000),
        (0x3ff0_0000_0000_0000, 0x3ff0_0000_0000_0000),
        (0x4000_0000_0000_0000, 0x3ff6_a09e_667f_3bcd),
        (0x4010_0000_0000_0000, 0x4000_0000_0000_0000),
        (0x3fd0_0000_0000_0000, 0x3fe0_0000_0000_0000),
        (0x0010_0000_0000_0000, 0x2000_0000_0000_0000),
    ] {
        for budget in [1, 3] {
            let (mut cpu, mut memory) = load(&[0xd9, 0xfa], input, 0);
            let mut expected_cpu = cpu;
            expected_cpu.eip += 14;
            for _ in 0..3 / budget {
                assert_eq!(cpu.run(&mut memory, budget).instructions, budget);
            }
            assert_eq!(result(&memory), expected);
            assert_eq!(cpu, expected_cpu);
        }
    }
}

#[test]
fn reverse_division_orders_memory_before_top_and_rounds_to_binary64() {
    for opcode in [0xd8, 0xdc] {
        for (top, source32, source64, expected) in [
            (
                2.0_f64.to_bits(),
                8.0_f32.to_bits(),
                8.0_f64.to_bits(),
                4.0_f64.to_bits(),
            ),
            (
                (-2.0_f64).to_bits(),
                8.0_f32.to_bits(),
                8.0_f64.to_bits(),
                (-4.0_f64).to_bits(),
            ),
            (
                3.0_f64.to_bits(),
                1.0_f32.to_bits(),
                1.0_f64.to_bits(),
                0x3fd5_5555_5555_5555,
            ),
            (
                3.0_f64.to_bits(),
                (-1.0_f32).to_bits(),
                (-1.0_f64).to_bits(),
                0xbfd5_5555_5555_5555,
            ),
            ((-2.0_f64).to_bits(), 0, 0, 0x8000_0000_0000_0000),
            ((-2.0_f64).to_bits(), 0x8000_0000, 0x8000_0000_0000_0000, 0),
        ] {
            let source = if opcode == 0xd8 {
                u64::from(source32)
            } else {
                source64
            };
            let (mut cpu, mut memory) = load(&[opcode, 0x3d, 0x10, 0x22, 0x40, 0], top, source);
            let mut expected_cpu = cpu;
            expected_cpu.eip += 18;
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(result(&memory), expected);
            assert_eq!(cpu, expected_cpu);
        }
    }
}

#[test]
fn excluded_math_and_range_boundaries_preserve_the_full_cpu() {
    let divide = &[0xdc, 0x3d, 0x10, 0x22, 0x40, 0][..];
    for (operation, top, source) in [
        (&[0xd9, 0xfa][..], -1.0_f64, 0.0_f64),
        (divide, 0.0, 1.0),
        (divide, -0.0, 0.0),
        (divide, f64::MIN_POSITIVE, f64::MAX),
        (divide, f64::MAX, f64::MIN_POSITIVE),
        (divide, 1.0, f64::MIN_POSITIVE),
        (divide, 1.0, (2.0 * f64::MIN_POSITIVE).next_down()),
        (divide, 1.0, f64::INFINITY),
        (divide, 1.0, f64::NAN),
        (divide, 1.0, f64::from_bits(1)),
    ] {
        let (mut cpu, mut memory) = load(operation, top.to_bits(), source.to_bits());
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(result(&memory), 0);
    }
    let (mut cpu, mut memory) = load(
        divide,
        1.0_f64.to_bits(),
        (2.0 * f64::MIN_POSITIVE).to_bits(),
    );
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(result(&memory), 0x0020_0000_0000_0000);
}

#[test]
fn math_checks_empty_stack_modes_and_memory_before_publishing_results() {
    for operation in [
        &[0xd9, 0xfa][..],
        &[0xd8, 0x3d, 0x10, 0x22, 0x40, 0],
        &[0xdc, 0x3d, 0x10, 0x22, 0x40, 0],
        &[0xd8, 0x0d, 0x10, 0x22, 0x40, 0],
        &[0xdc, 0x0d, 0x10, 0x22, 0x40, 0],
    ] {
        let (mut cpu, mut memory) = load(operation, 4.0_f64.to_bits(), 0);
        cpu.eip += 6;
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        cpu.eip -= 6;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        cpu.set_x87_control_word(0x037f);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    for (mode, address) in [
        (0x3d, 0x0040_2ffc_u32),
        (0x3d, 0xffff_fffc),
        (0x0d, 0x0040_2ffc),
        (0x0d, 0xffff_fffc),
    ] {
        let mut operation = vec![0xdc, mode];
        operation.extend_from_slice(&address.to_le_bytes());
        let (mut cpu, mut memory) = load(&operation, 4.0_f64.to_bits(), 0);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
    }
}
