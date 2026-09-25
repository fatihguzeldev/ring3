#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2300;

fn instruction(opcode: u8, mode: u8, address: u32) -> Vec<u8> {
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

fn read(memory: &GuestMemory, address: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

#[test]
fn memory_transfers_preserve_finite_bits_and_signed_zero() {
    for (opcode, inputs) in [
        (
            0xd9,
            vec![
                0,
                0x8000_0000,
                0x3f80_0000,
                0xbf80_0000,
                0x0080_0000,
                0x7f7f_ffff,
            ],
        ),
        (
            0xdd,
            vec![
                0,
                0x8000_0000_0000_0000,
                0x3ff0_0000_0000_0000,
                0xbff0_0000_0000_0000,
                0x0010_0000_0000_0000,
                0x7fef_ffff_ffff_ffff,
            ],
        ),
    ] {
        let size = if opcode == 0xd9 { 4 } else { 8 };
        let mut code = instruction(opcode, 0x05, INPUT);
        code.extend(instruction(opcode, 0x15, OUTPUT));
        code.extend(instruction(opcode, 0x1d, OUTPUT + 16));
        for value in inputs {
            let (mut cpu, mut memory) = load(&code);
            let input = u64::to_le_bytes(value);
            memory.write(u64::from(INPUT), &input[..size]).unwrap();
            memory.write(u64::from(OUTPUT), &[0x55; 32]).unwrap();
            let result = cpu.run(&mut memory, 3);
            assert_eq!(result.reason, StopReason::InstructionLimit);
            assert_eq!(result.instructions, 3);
            assert_eq!(read(&memory, OUTPUT, size), input[..size]);
            assert_eq!(read(&memory, OUTPUT + 16, size), input[..size]);
            assert_eq!(
                read(&memory, OUTPUT + u32::try_from(size).unwrap(), 1),
                [0x55]
            );
            assert_eq!(cpu.eflags, 0xced7);
            assert_eq!(cpu.x87_control_word(), 0x027f);
        }
    }
}

#[test]
fn masked_double_nan_load_quiets_signaling_input_and_sets_invalid_status() {
    let mut code = instruction(0xdd, 0x05, INPUT);
    code.extend([0xdf, 0xe0]);
    code.extend(instruction(0xdd, 0x1d, OUTPUT));
    for (source, expected, invalid_status) in [
        (0xfff0_0000_0000_0001_u64, 0xfff8_0000_0000_0001_u64, 1),
        (0x7ff8_0000_0000_0011, 0x7ff8_0000_0000_0011, 0),
    ] {
        let (mut cpu, mut memory) = load(&code);
        memory
            .write(u64::from(INPUT), &source.to_le_bytes())
            .unwrap();
        memory.write(u64::from(OUTPUT), &[0x55; 8]).unwrap();
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 1, invalid_status);
        assert_eq!(read(&memory, OUTPUT, 8), expected.to_le_bytes());
        assert_eq!(read(&memory, INPUT, 8), source.to_le_bytes());
        assert_eq!(cpu.eflags, 0xced7);
    }

    let (mut cpu, mut memory) = load(&instruction(0xdd, 0x05, INPUT));
    let entry = cpu.eip;
    memory
        .write(u64::from(INPUT), &1_f64.to_le_bytes())
        .unwrap();
    for _ in 0..8 {
        cpu.eip = entry;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    }
    memory
        .write(u64::from(INPUT), &0xfff0_0000_0000_0001_u64.to_le_bytes())
        .unwrap();
    cpu.eip = entry;
    let full = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, full);
}

#[test]
fn masked_double_denormal_load_keeps_bits_and_sets_denormal_status() {
    let mut code = instruction(0xdd, 0x05, INPUT);
    code.extend([0xdf, 0xe0]);
    code.extend(instruction(0xdd, 0x1d, OUTPUT));
    for source in [1_u64, 0x000f_ffff_ffff_ffff, 0x8000_0000_0000_0001] {
        let (mut cpu, mut memory) = load(&code);
        memory
            .write(u64::from(INPUT), &source.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 2, 2);
        assert_eq!(read(&memory, OUTPUT, 8), source.to_le_bytes());
    }
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &1_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax) & 2, 0);
}

#[test]
fn narrowing_uses_nearest_even_and_rejects_unsupported_ranges() {
    let mut code = instruction(0xdd, 0x05, INPUT);
    code.extend(instruction(0xd9, 0x1d, OUTPUT));
    for (value, expected) in [
        (0x3ff0_0000_1000_0000_u64, 0x3f80_0000_u32),
        (0x3ff0_0000_3000_0000, 0x3f80_0002),
        (0xbff0_0000_1000_0000, 0xbf80_0000),
        (0xbff0_0000_3000_0000, 0xbf80_0002),
    ] {
        let (mut cpu, mut memory) = load(&code);
        memory
            .write(u64::from(INPUT), &value.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(read(&memory, OUTPUT, 4), expected.to_le_bytes());
    }
    for value in [
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        f64::MAX,
        f64::from(f32::MIN_POSITIVE).next_down(),
        f64::from(f32::MAX).next_up(),
    ] {
        let (mut cpu, mut memory) = load(&code);
        memory
            .write(u64::from(INPUT), &value.to_le_bytes())
            .unwrap();
        memory.write(u64::from(OUTPUT), &[0x55; 8]).unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(read(&memory, OUTPUT, 8), [0x55; 8]);
    }
}

#[test]
fn eight_slots_are_lifo_and_stack_faults_do_not_mutate_state() {
    let mut code = instruction(0xd9, 0x05, INPUT);
    code.extend(instruction(0xd9, 0x1d, OUTPUT));
    let (mut cpu, mut memory) = load(&code);
    let entry = cpu.eip;
    let empty = cpu;
    assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
    assert_eq!(cpu, empty);
    for i in 1..=8_u32 {
        memory
            .write(u64::from(INPUT), &(0x3f80_0000 + i).to_le_bytes())
            .unwrap();
        cpu.eip = entry;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    }
    cpu.eip = entry;
    let full = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, full);
    for i in (1..=8_u32).rev() {
        cpu.eip = entry + 6;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(read(&memory, OUTPUT, 4), (0x3f80_0000 + i).to_le_bytes());
    }
    cpu.eip = entry;
    assert_eq!(cpu, empty);
    cpu.eip = entry + 6;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn loads_reject_special_values_and_excluded_control_modes() {
    for (opcode, values) in [
        (
            0xd9,
            vec![
                1_u64,
                0x007f_ffff,
                0x7f80_0000,
                0xff80_0000,
                0x7fc0_0000,
                0x7f80_0001,
            ],
        ),
        (0xdd, vec![0x7ff0_0000_0000_0000, 0xfff0_0000_0000_0000]),
    ] {
        for value in values {
            let (mut cpu, mut memory) = load(&instruction(opcode, 0x05, INPUT));
            memory
                .write(u64::from(INPUT), &value.to_le_bytes())
                .unwrap();
            let before = cpu;
            assert_eq!(
                cpu.run(&mut memory, 1).reason,
                StopReason::UnsupportedInstruction
            );
            assert_eq!(cpu, before);
        }
    }
    for control in [0x027e, 0x027d, 0x027b, 0x0277, 0x026f, 0x025f] {
        let (mut cpu, mut memory) = load(&instruction(0xd9, 0x05, INPUT));
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
fn memory_faults_and_store_retry_preserve_the_stack_and_full_output() {
    for (opcode, size) in [(0xd9, 4_u32), (0xdd, 8)] {
        for address in [0x0040_3000 - size + 1, u32::MAX - size + 2] {
            let (mut cpu, mut memory) = load(&instruction(opcode, 0x05, address));
            let before = cpu;
            assert!(matches!(
                cpu.run(&mut memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        }
        let output = 0x0040_3000 - size + 1;
        let mut code = instruction(opcode, 0x05, INPUT);
        code.extend(instruction(opcode, 0x1d, output));
        let (mut cpu, mut memory) = load(&code);
        let data = if size == 4 {
            0x3f80_0000_u64
        } else {
            0x3ff0_0000_0000_0000
        };
        memory.write(u64::from(INPUT), &data.to_le_bytes()).unwrap();
        memory
            .map_zeroed(0x0040_3000, 4096, Permissions::READ)
            .unwrap();
        memory
            .write(u64::from(output), &vec![0x55; size as usize - 1])
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
        assert_eq!(
            read(&memory, output, size as usize - 1),
            vec![0x55; size as usize - 1]
        );
        assert_eq!(read(&memory, 0x0040_3000, 1), [0]);
        memory
            .protect(0x0040_3000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(
            read(&memory, output, size as usize),
            data.to_le_bytes()[..size as usize]
        );
        cpu.eip = before.eip;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
    }
}

#[test]
fn fs_addressing_aliases_and_last_address_span_work() {
    let mut code = vec![0x64];
    code.extend(instruction(0xd9, 0x05, 0x200));
    code.push(0x64);
    code.extend(instruction(0xd9, 0x1d, 0x200));
    let (mut cpu, mut memory) = load(&code);
    cpu.set_fs_base(0x0040_2000);
    memory
        .write(u64::from(INPUT), &0xbf80_0000_u32.to_le_bytes())
        .unwrap();
    let mut expected = cpu;
    expected.eip += 14;
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(cpu, expected);
    assert_eq!(read(&memory, INPUT, 4), 0xbf80_0000_u32.to_le_bytes());
    for opcode in [0xd9, 0xdd] {
        let size = if opcode == 0xd9 { 4 } else { 8 };
        let address = u32::MAX - size + 1;
        let mut code = instruction(opcode, 0x05, address);
        code.extend(instruction(opcode, 0x1d, address));
        let (mut cpu, mut memory) = load(&code);
        memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(
            read(&memory, address, size as usize),
            vec![0; size as usize]
        );
    }
}

#[test]
fn examination_and_unimplemented_stack_forms_remain_explicit_stops() {
    for code in [
        vec![0xd9, 0xe5],
        vec![0xd9, 0xe4],
        vec![0xd9, 0xc0],
        vec![0xd9, 0xe9],
        vec![0xdb, 0xe2],
    ] {
        let (mut cpu, mut memory) = load(&code);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}
