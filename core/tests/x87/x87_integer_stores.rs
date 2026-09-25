use super::executable;
use super::integer_cpu_state;
use super::integer_store_executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2300;
const FORMS: [(u8, u8, usize, bool); 5] = [
    (0xdf, 0x15, 2, false),
    (0xdb, 0x15, 4, false),
    (0xdf, 0x1d, 2, true),
    (0xdb, 0x1d, 4, true),
    (0xdf, 0x3d, 8, true),
];

fn operand(opcode: u8, mode: u8, address: u32) -> Vec<u8> {
    let mut code = vec![opcode, mode];
    code.extend_from_slice(&address.to_le_bytes());
    code
}

fn load(opcode: u8, mode: u8, address: u32, input: f64, control: u16) -> (Cpu32, GuestMemory) {
    let mut code = operand(0xdd, 0x05, INPUT);
    code.extend(operand(opcode, mode, address));
    code.extend([0xdf, 0xe0]);
    let mut image = load_pe32(&executable::pe32(&code), 32).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &input.to_le_bytes())
        .unwrap();
    image.memory.write(u64::from(OUTPUT), &[0xaa; 16]).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn read(memory: &GuestMemory, address: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

#[test]
fn authored_three_width_conversion_agrees_whole_or_stepwise() {
    for budget in [1, 20] {
        let mut image = load_pe32(&integer_store_executable::pe32(), 4).unwrap();
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
        assert_eq!(count, 6);
        let mut bytes = [0; 16];
        image.memory.read(0x0040_21a0, &mut bytes).unwrap();
        assert_eq!(&bytes[..4], &[0xff, 0xff, 0xaa, 0xaa]);
        assert_eq!(&bytes[4..], &[0xff; 12]);
    }
}

#[test]
fn every_store_form_uses_guest_rounding_and_changes_only_its_output_and_optional_pop() {
    for (input, expected) in [
        (0.0_f64, [0_i64; 4]),
        (-0.0, [0; 4]),
        (0.5, [0, 0, 1, 0]),
        (-0.5, [0, -1, 0, 0]),
        (1.5, [2, 1, 2, 1]),
        (-1.5, [-2, -2, -1, -1]),
        (2.5, [2, 2, 3, 2]),
        (-2.5, [-2, -3, -2, -2]),
        (1.75, [2, 1, 2, 1]),
        (-1.75, [-2, -2, -1, -1]),
        (f64::MIN_POSITIVE, [0, 0, 1, 0]),
        (-f64::MIN_POSITIVE, [0, -1, 0, 0]),
    ] {
        for (opcode, mode, size, pop) in FORMS {
            for (rc, expected) in expected.into_iter().enumerate() {
                let control = 0x027f | (u16::try_from(rc).unwrap() << 10);
                let (mut cpu, mut memory) = load(opcode, mode, OUTPUT, input, control);
                let empty = cpu;
                assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                let mut expected_cpu = if pop { empty } else { cpu };
                expected_cpu.eip = empty.eip + 12;
                let before = cpu;
                assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
                assert_eq!(cpu, before);
                assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                integer_cpu_state::assert_unchanged(&cpu, &expected_cpu);
                assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                let rounded = f64::from(i32::try_from(expected).unwrap());
                let status = if pop { 0 } else { 0x3800 }
                    | if input == 0.0 { 0 } else { 0x20 }
                    | if rounded.abs() > input.abs() {
                        0x200
                    } else {
                        0
                    };
                assert_eq!(cpu.register(Register32::Eax), status);
                assert_eq!(read(&memory, OUTPUT, size), expected.to_le_bytes()[..size]);
                assert_eq!(
                    read(&memory, OUTPUT + u32::try_from(size).unwrap(), 16 - size),
                    vec![0xaa; 16 - size]
                );
            }
        }
    }
}

#[test]
fn destination_ranges_are_checked_after_rounding_without_indefinite_outputs() {
    for (opcode, mode, size, _) in FORMS {
        let cases = match size {
            2 => vec![
                (32767.5, [None, Some(32767_i64), None, Some(32767)]),
                (-32768.5, [Some(-32768), None, Some(-32768), Some(-32768)]),
            ],
            4 => vec![
                (
                    2_147_483_647.5,
                    [None, Some(2_147_483_647), None, Some(2_147_483_647)],
                ),
                (
                    -2_147_483_648.5,
                    [
                        Some(-2_147_483_648),
                        None,
                        Some(-2_147_483_648),
                        Some(-2_147_483_648),
                    ],
                ),
            ],
            _ => vec![
                (9_223_372_036_854_775_808.0, [None; 4]),
                (-9_223_372_036_854_775_808.0, [Some(i64::MIN); 4]),
                (
                    f64::from_bits(0x43df_ffff_ffff_ffff),
                    [Some(0x7fff_ffff_ffff_fc00); 4],
                ),
                (f64::from_bits(0xc3e0_0000_0000_0001), [None; 4]),
            ],
        };
        for (input, expected) in cases {
            for (rc, expected) in expected.into_iter().enumerate() {
                let control = 0x027f | (u16::try_from(rc).unwrap() << 10);
                let (mut cpu, mut memory) = load(opcode, mode, OUTPUT, input, control);
                assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                let before = cpu;
                let run = cpu.run(&mut memory, 1);
                if let Some(integer) = expected {
                    assert_eq!(run.instructions, 1);
                    assert_eq!(read(&memory, OUTPUT, size), integer.to_le_bytes()[..size]);
                } else {
                    assert_eq!(
                        (run.reason, run.instructions),
                        (StopReason::UnsupportedInstruction, 0)
                    );
                    assert_eq!(cpu, before);
                    assert_eq!(read(&memory, OUTPUT, 16), vec![0xaa; 16]);
                }
            }
        }
    }
}

#[test]
fn exact_loads_ignore_pc_rc_while_arithmetic_and_float_stores_keep_their_profile() {
    for (opcode, mode, input) in [
        (0xd9, 0x05, u64::from(7.0_f32.to_bits())),
        (0xdd, 0x05, 7.0_f64.to_bits()),
        (0xdf, 0x05, 7),
        (0xdb, 0x05, 7),
        (0xdf, 0x2d, 7),
    ] {
        for pc in [0, 0x200, 0x300] {
            for rc in [0, 0x400, 0x800, 0xc00] {
                let mut code = operand(opcode, mode, INPUT);
                code.extend(operand(0xdb, 0x1d, OUTPUT));
                let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
                image
                    .memory
                    .write(u64::from(INPUT), &input.to_le_bytes())
                    .unwrap();
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_x87_control_word(0x7f | pc | rc);
                assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
                assert_eq!(read(&image.memory, OUTPUT, 4), 7_i32.to_le_bytes());
            }
        }
    }
    for (opcode, mode) in [(0xd9, 0x1d), (0xdd, 0x1d), (0xdc, 0x0d)] {
        let (mut cpu, mut memory) = load(opcode, mode, OUTPUT, 7.0, 0x0e7f);
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
fn split_output_faults_and_retry_do_not_pop_or_partially_write() {
    for (opcode, mode, size, _) in FORMS {
        let address = 0x0040_3000 - u32::try_from(size).unwrap() + 1;
        let (mut cpu, mut memory) = load(opcode, mode, address, -1.75, 0x0e7f);
        memory
            .write(u64::from(address), &vec![0xaa; size - 1])
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        for mapped in [false, true] {
            if mapped {
                memory
                    .map_zeroed(0x0040_3000, 4096, Permissions::READ)
                    .unwrap();
            }
            let before = cpu;
            let run = cpu.run(&mut memory, 1);
            assert!(matches!(run.reason, StopReason::MemoryFault(_)));
            assert_eq!(run.instructions, 0);
            assert_eq!(cpu, before);
            assert_eq!(read(&memory, address, size - 1), vec![0xaa; size - 1]);
        }
        memory
            .protect(0x0040_3000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(read(&memory, address, size), vec![0xff; size]);
    }
}

#[test]
fn fs_output_last_address_and_source_alias_preserve_stack_semantics() {
    for (opcode, mode, size, _) in FORMS {
        let address = u32::MAX - u32::try_from(size).unwrap() + 1;
        let (mut cpu, mut memory) = load(opcode, mode, address, -1.75, 0x0e7f);
        memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(read(&memory, address, size), vec![0xff; size]);
        let (mut cpu, mut memory) = load(opcode, mode, address + 1, -1.75, 0x0e7f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
    }
    let mut code = operand(0xdd, 0x05, INPUT);
    code.push(0x64);
    code.extend(operand(0xdb, 0x15, 0x200));
    code.extend(operand(0xdb, 0x1d, OUTPUT));
    let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &7.75_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x0e7f);
    cpu.set_fs_base(0x0040_2000);
    assert_eq!(cpu.run(&mut image.memory, 3).instructions, 3);
    assert_eq!(read(&image.memory, INPUT, 4), 7_i32.to_le_bytes());
    assert_eq!(read(&image.memory, OUTPUT, 4), 7_i32.to_le_bytes());
}

#[test]
fn empty_stack_unmasked_exceptions_and_truncating_opcode_remain_explicit_stops() {
    for (opcode, mode, _, _) in FORMS {
        let (mut cpu, mut memory) = load(opcode, mode, OUTPUT, 1.75, 0x027f);
        cpu.eip += 6;
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        cpu.eip -= 6;
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        for bit in 0..6 {
            cpu.set_x87_control_word(0x027f & !(1 << bit));
            let before = cpu;
            assert_eq!(
                cpu.run(&mut memory, 1).reason,
                StopReason::UnsupportedInstruction
            );
            assert_eq!(cpu, before);
            assert_eq!(read(&memory, OUTPUT, 16), vec![0xaa; 16]);
        }
    }
    for opcode in [0xdf, 0xdb, 0xdd] {
        let (mut cpu, mut memory) = load(opcode, 0x0d, OUTPUT, 1.75, 0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}
