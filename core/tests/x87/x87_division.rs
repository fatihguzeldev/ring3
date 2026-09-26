use super::division_executable;
use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

fn memory_instruction(opcode: u8, mode: u8, address: u32) -> Vec<u8> {
    let mut code = vec![opcode, mode];
    code.extend_from_slice(&address.to_le_bytes());
    code
}

fn load(opcode: u8, numerator: u64, source: u64, address: u32) -> (Cpu32, GuestMemory) {
    let mut code = memory_instruction(0xdd, 0x05, 0x0040_2200);
    code.extend(memory_instruction(opcode, 0x35, address));
    code.extend([0xdf, 0xe0]);
    code.extend(memory_instruction(0xdd, 0x1d, 0x0040_2300));
    let image = load_pe32(&executable::pe32(&code), 32).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.set_register(Register32::Eax, 0xabcd_1234);
    cpu.eflags = 0xced7;
    let mut memory = image.memory;
    memory.write(0x0040_2200, &numerator.to_le_bytes()).unwrap();
    memory.write(0x0040_2210, &source.to_le_bytes()).unwrap();
    (cpu, memory)
}

fn load_single_code(numerator: f64, divisor: f32, mode: u8) -> (Cpu32, GuestMemory) {
    let mut code = memory_instruction(0xdd, 0x05, 0x0040_2200);
    code.extend(memory_instruction(0xd8, mode, 0x0040_2210));
    code.extend([0xdf, 0xe0]);
    code.extend(memory_instruction(0xd9, 0x1d, 0x0040_2300));
    let mut image = load_pe32(&executable::pe32(&code), 32).unwrap();
    image
        .memory
        .write(0x0040_2200, &numerator.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(0x0040_2210, &divisor.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    (cpu, image.memory)
}

fn load_single(numerator: f64, divisor: f32) -> (Cpu32, GuestMemory) {
    load_single_code(numerator, divisor, 0x35)
}

fn load_single_reverse_m64(
    top: f64,
    numerator: f64,
    control: u16,
    address: u32,
) -> (Cpu32, GuestMemory) {
    let mut code = memory_instruction(0xdd, 0x05, 0x0040_2200);
    code.extend(memory_instruction(0xdc, 0x3d, address));
    code.extend([0xdf, 0xe0]);
    code.extend(memory_instruction(0xd9, 0x1d, 0x0040_2300));
    let mut image = load_pe32(&executable::pe32(&code), 32).unwrap();
    image.memory.write(0x0040_2200, &top.to_le_bytes()).unwrap();
    image
        .memory
        .write(0x0040_2210, &numerator.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

#[test]
fn single_precision_reverse_m64_division_rounds_and_keeps_its_source() {
    for (top, numerator, expected, status) in [
        (3.0, 6.0, 2.0_f32, 0),
        (3.0, 1.0, 1.0_f32 / 3.0, 0x220),
        (31.0, 1.0, 1.0_f32 / 31.0, 0x20),
        (3.0, -1.0, -1.0_f32 / 3.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load_single_reverse_m64(top, numerator, 0x007f, 0x0040_2210);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut output = [0; 4];
        memory.read(0x0040_2300, &mut output).unwrap();
        assert_eq!(u32::from_le_bytes(output), expected.to_bits());
        let mut source = [0; 8];
        memory.read(0x0040_2210, &mut source).unwrap();
        assert_eq!(u64::from_le_bytes(source), numerator.to_bits());
    }
}

#[test]
fn single_precision_reverse_m64_division_rejects_invalid_and_faults_atomically() {
    for (top, numerator, control, address) in [
        (0.0, 1.0, 0x007f, 0x0040_2210),
        (0.5, f64::from(f32::MAX) * 2.0, 0x007f, 0x0040_2210),
        (2.0, f64::from(f32::MIN_POSITIVE), 0x007f, 0x0040_2210),
        (2.0, 1.0, 0x0c7f, 0x0040_2210),
        (2.0, 1.0, 0x007f, 0x6000_0000),
    ] {
        let (mut cpu, mut memory) = load_single_reverse_m64(top, numerator, control, address);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        let reason = cpu.run(&mut memory, 1).reason;
        if address == 0x6000_0000 {
            assert!(matches!(reason, StopReason::MemoryFault(_)));
        } else {
            assert_eq!(reason, StopReason::UnsupportedInstruction);
        }
        assert_eq!(cpu, before);
    }
}

#[test]
fn single_precision_reverse_memory_division_uses_source_over_top() {
    for (top, source, expected, status) in [
        (3.0, 6.0_f32, 2.0_f32, 0),
        (3.0, 1.0_f32, 1.0_f32 / 3.0, 0x220),
        (31.0, 1.0_f32, 1.0_f32 / 31.0, 0x20),
        (3.0, -1.0_f32, -1.0_f32 / 3.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load_single_code(top, source, 0x3d);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut bytes = [0; 4];
        memory.read(0x0040_2300, &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), expected.to_bits());
        memory.read(0x0040_2210, &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), source.to_bits());
    }
}

#[test]
fn single_precision_reverse_memory_division_rejects_invalid_results_atomically() {
    for (top, source, control) in [
        (0.0, 1.0_f32, 0x007f),
        (0.5, f32::MAX, 0x007f),
        (2.0, f32::MIN_POSITIVE, 0x007f),
        (2.0, 1.0_f32, 0x0c7f),
    ] {
        let (mut cpu, mut memory) = load_single_code(top, source, 0x3d);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn single_precision_memory_division_rounds_and_keeps_the_stack() {
    for (numerator, divisor, expected, status) in [
        (8.0, 2.0, 4.0_f32, 0),
        (1.0, 3.0, 1.0_f32 / 3.0, 0x220),
        (1.0, 31.0, 1.0_f32 / 31.0, 0x20),
        (-1.0, 3.0, -1.0_f32 / 3.0, 0x220),
    ] {
        let (mut cpu, mut memory) = load_single(numerator, divisor);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x3a20, 0x3800 | status);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let mut output = [0; 4];
        memory.read(0x0040_2300, &mut output).unwrap();
        assert_eq!(u32::from_le_bytes(output), expected.to_bits());
        let mut input = [0; 4];
        memory.read(0x0040_2210, &mut input).unwrap();
        assert_eq!(u32::from_le_bytes(input), divisor.to_bits());
    }
}

#[test]
fn single_precision_memory_division_rejects_invalid_results_atomically() {
    for (numerator, divisor) in [
        (1.0, 0.0),
        (f64::from(f32::MAX) * 2.0, 1.0),
        (f64::from(f32::MIN_POSITIVE), 2.0),
    ] {
        let (mut cpu, mut memory) = load_single(numerator, divisor);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn normal_division_executes_both_widths_whole_or_stepwise() {
    for budget in [1, 40] {
        let image = load_pe32(&division_executable::pe32(), 32).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut memory = image.memory;
        let mut count = 0;
        loop {
            let run = cpu.run(&mut memory, budget);
            count += run.instructions;
            if run.reason != StopReason::InstructionLimit {
                assert_eq!(run.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 20);
        }
        assert_eq!(count, 9);
        assert_eq!(cpu.register(Register32::Esi), 0x3800);
        assert_eq!(cpu.register(Register32::Eax), 0x20);
        let mut bytes = [0; 8];
        memory.read(0x0040_21a0, &mut bytes).unwrap();
        assert_eq!(u64::from_le_bytes(bytes), 0x3ff5_5555_5555_5555);
    }
}

#[test]
fn both_widths_order_operands_preserve_zero_signs_and_report_rounding() {
    for opcode in [0xd8, 0xdc] {
        for (numerator, divisor, expected, status) in [
            (6_f64, 3_f32, 2_f64.to_bits(), 0),
            (3., 6., 0.5_f64.to_bits(), 0),
            (1., 3., 0x3fd5_5555_5555_5555, 0x20),
            (1., 10., 0x3fb9_9999_9999_999a, 0x220),
            (-1., 10., 0xbfb9_9999_9999_999a, 0x220),
            (1., -10., 0xbfb9_9999_9999_999a, 0x220),
            (0., 2., 0, 0),
            (-0., 2., 0x8000_0000_0000_0000, 0),
            (0., -2., 0x8000_0000_0000_0000, 0),
            (-0., -2., 0, 0),
        ] {
            let source = if opcode == 0xd8 {
                u64::from(divisor.to_bits())
            } else {
                f64::from(divisor).to_bits()
            };
            let (mut cpu, mut memory) = load(opcode, numerator.to_bits(), source, 0x0040_2210);
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_3800 | status);
            assert_eq!(cpu.eflags, 0xced7);
            assert_eq!(cpu.x87_control_word(), 0x027f);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let mut output = [0; 8];
            memory.read(0x0040_2300, &mut output).unwrap();
            assert_eq!(u64::from_le_bytes(output), expected);
        }
    }
}

#[test]
fn excluded_divisors_ranges_and_control_modes_preserve_the_loaded_operand() {
    for opcode in [0xd8, 0xdc] {
        for (bits32, bits64) in [
            (0_u32, 0_u64),
            (0x8000_0000, 0x8000_0000_0000_0000),
            (0x7fc0_0000, 0x7ff8_0000_0000_0000),
            (0x7f80_0000, 0x7ff0_0000_0000_0000),
        ] {
            let source = if opcode == 0xd8 {
                u64::from(bits32)
            } else {
                bits64
            };
            let (mut cpu, mut memory) = load(opcode, 1_f64.to_bits(), source, 0x0040_2210);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let before = cpu;
            assert_eq!(
                cpu.run(&mut memory, 1).reason,
                StopReason::UnsupportedInstruction
            );
            assert_eq!(cpu, before);
        }
    }
    for (top, divisor) in [
        (1., f64::from_bits(1)),
        (f64::MAX, 0.5),
        (f64::MIN_POSITIVE, 2.),
        (f64::MIN_POSITIVE, 1.),
        (1., f64::MAX),
    ] {
        let (mut cpu, mut memory) = load(0xdc, top.to_bits(), divisor.to_bits(), 0x0040_2210);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    for control in [0x007f, 0x037f, 0x067f, 0x0a7f, 0x0e7f, 0x027e] {
        let (mut cpu, mut memory) = load(0xdc, 1_f64.to_bits(), 2_f64.to_bits(), 0x0040_2210);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        cpu.set_x87_control_word(control);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn full_read_spans_and_retry_preserve_stack_status_and_input_bytes() {
    for opcode in [0xd8, 0xdc] {
        for address in [0x6000_0000, 0x0040_2fff, u32::MAX] {
            let (mut cpu, mut memory) = load(opcode, 1_f64.to_bits(), 2_f64.to_bits(), address);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let before = cpu;
            assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
            assert_eq!(cpu, before);
            assert!(matches!(
                cpu.run(&mut memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        }
    }
    let (mut cpu, mut memory) = load(0xdc, 6_f64.to_bits(), 3_f64.to_bits(), 0x0040_2210);
    let entry = cpu.eip;
    cpu.eip += 6;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    cpu.eip = entry;
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    let mut bytes = [0; 8];
    memory.read(0x0040_2300, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 2_f64.to_bits());
    memory.read(0x0040_2210, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 3_f64.to_bits());
}

#[test]
fn fs_divisors_and_smallest_admitted_results_keep_the_existing_profile() {
    let mut code = memory_instruction(0xdd, 0x05, 0x0040_2200);
    code.push(0x64);
    code.extend(memory_instruction(0xdc, 0x35, 0xffff_ffe0));
    code.extend(memory_instruction(0xdd, 0x1d, 0x0040_2300));
    let image = load_pe32(&executable::pe32(&code), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let mut memory = image.memory;
    cpu.set_x87_control_word(0x027f);
    cpu.set_fs_base(0x0040_2230);
    memory
        .write(0x0040_2200, &(4. * f64::MIN_POSITIVE).to_le_bytes())
        .unwrap();
    memory.write(0x0040_2210, &2_f64.to_le_bytes()).unwrap();
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    let mut bytes = [0; 8];
    memory.read(0x0040_2300, &mut bytes).unwrap();
    assert_eq!(u64::from_le_bytes(bytes), 0x0020_0000_0000_0000);
}
