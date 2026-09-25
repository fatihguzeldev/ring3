use super::executable;
use super::string_moves_executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

fn encoding(width: u32) -> Vec<u8> {
    match width {
        1 => vec![0xa4],
        2 => vec![0x66, 0xa5],
        _ => vec![0xa5],
    }
}

#[test]
fn all_widths_copy_one_element_and_update_only_indices_in_either_direction() {
    for width in [1, 2, 4_u32] {
        for reverse in [false, true] {
            for fs in [false, true] {
                for count in [0, 7] {
                    let mut code = encoding(width);
                    if fs {
                        code.insert(0, 0x64);
                    }
                    let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
                    image
                        .memory
                        .map_zeroed(0x5000, PAGE_SIZE, Permissions::READ_WRITE)
                        .unwrap();
                    image
                        .memory
                        .write(0x0040_2081, &[0x12, 0x34, 0x56, 0x78])
                        .unwrap();
                    image.memory.write(0x5080, &[0xaa; 6]).unwrap();
                    image
                        .memory
                        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
                        .unwrap();
                    image
                        .memory
                        .protect(
                            0x5000,
                            PAGE_SIZE,
                            Permissions {
                                read: false,
                                write: true,
                                execute: false,
                            },
                        )
                        .unwrap();
                    let mut cpu = Cpu32::new(image.entry_point);
                    cpu.set_fs_base(0x0040_2000);
                    cpu.set_register(Register32::Esi, if fs { 0x81 } else { 0x0040_2081 });
                    cpu.set_register(Register32::Edi, 0x5081);
                    cpu.set_register(Register32::Ecx, count);
                    cpu.eflags = (0xced7 & !0x400) | if reverse { 0x400 } else { 0 };
                    let mut expected = cpu;
                    let delta = if reverse { width.wrapping_neg() } else { width };
                    for reg in [Register32::Esi, Register32::Edi] {
                        expected.set_register(reg, cpu.register(reg).wrapping_add(delta));
                    }
                    expected.eip += u32::try_from(code.len()).unwrap();
                    let result = cpu.run(&mut image.memory, 1);
                    assert_eq!(result.reason, StopReason::InstructionLimit);
                    assert_eq!(result.instructions, 1);
                    assert_eq!(cpu, expected);
                    image
                        .memory
                        .protect(0x5000, PAGE_SIZE, Permissions::READ)
                        .unwrap();
                    let mut actual = [0; 6];
                    image.memory.read(0x5080, &mut actual).unwrap();
                    let mut wanted = [0xaa; 6];
                    wanted[1..][..width as usize]
                        .copy_from_slice(&[0x12, 0x34, 0x56, 0x78][..width as usize]);
                    assert_eq!(actual, wanted);
                }
            }
        }
    }
}

#[test]
fn overlapping_elements_capture_source_before_writing_and_indices_wrap() {
    for width in [1, 2, 4_u32] {
        let mut image = load_pe32(&executable::pe32(&encoding(width)), 5).unwrap();
        image
            .memory
            .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        for (source, destination, reverse) in [
            (0x0040_2080, 0x0040_2081, false),
            (0x0040_2081, 0x0040_2080, true),
            (u32::MAX - width + 1, 0x0040_2080, false),
            (0, 0x0040_2080, true),
            (0x0040_2080, u32::MAX - width + 1, false),
        ] {
            image
                .memory
                .write(u64::from(source), &[1, 2, 3, 4][..width as usize])
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Esi, source);
            cpu.set_register(Register32::Edi, destination);
            cpu.eflags = if reverse { 0x402 } else { 2 };
            let mut expected = cpu;
            let delta = if reverse { width.wrapping_neg() } else { width };
            expected.set_register(Register32::Esi, source.wrapping_add(delta));
            expected.set_register(Register32::Edi, destination.wrapping_add(delta));
            expected.eip += u32::try_from(encoding(width).len()).unwrap();
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            assert_eq!(cpu, expected);
            let mut bytes = [0; 4];
            image
                .memory
                .read(u64::from(destination), &mut bytes[..width as usize])
                .unwrap();
            assert_eq!(&bytes[..width as usize], &[1, 2, 3, 4][..width as usize]);
        }
    }
}

#[test]
fn source_and_destination_faults_do_not_copy_partial_elements_or_update_cpu() {
    for (source, destination, no_read, read_only) in [
        (0x0040_2fff_u32, 0x5080, false, false),
        (u32::MAX, 0x5080, false, false),
        (0x0040_2080, 0x5fff, false, false),
        (0x0040_2080, u32::MAX, false, false),
        (0x0040_2080, 0x5080, true, false),
        (0x0040_2080, 0x5080, false, true),
    ] {
        let mut image = load_pe32(&executable::pe32(&[0xa5]), 5).unwrap();
        image
            .memory
            .map_zeroed(0x5000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        image.memory.write(u64::from(destination), &[0xaa]).unwrap();
        image.memory.write(0x0040_2080, &[1, 2, 3, 4]).unwrap();
        if no_read {
            image
                .memory
                .protect(0x0040_2000, PAGE_SIZE, Permissions::NONE)
                .unwrap();
        }
        if read_only {
            image
                .memory
                .protect(0x5000, PAGE_SIZE, Permissions::READ)
                .unwrap();
        }
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Esi, source);
        cpu.set_register(Register32::Edi, destination);
        cpu.eflags = 0xced7;
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
        let mut byte = [0];
        image
            .memory
            .read(u64::from(destination), &mut byte)
            .unwrap();
        assert_eq!(byte, [0xaa]);
    }
    for code in [
        &[0x67, 0xa5][..],
        &[0xf3, 0x67, 0xa5],
        &[0xf2, 0xa4],
        &[0x65, 0xa5],
        &[0xf0, 0xa5],
        &[0xf2, 0x0f, 0x10, 0xc0],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert_eq!(
            result.reason,
            if code[0] == 0xf0 {
                StopReason::InvalidInstruction
            } else {
                StopReason::UnsupportedInstruction
            }
        );
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn string_copy_guest_matches_whole_and_single_instruction_execution() {
    let bytes = string_moves_executable::pe32();
    let mut whole = load_pe32(&bytes, 3).unwrap();
    let mut stepped = load_pe32(&bytes, 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.eflags = 0xcad7;
    let mut other = cpu;
    let result = cpu.run(&mut whole.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 6);
    let mut count = 0;
    for _ in 0..20 {
        let step = other.run(&mut stepped.memory, 1);
        count += step.instructions;
        if step.reason != StopReason::InstructionLimit {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(count, 6);
    assert_eq!(other, cpu);
    assert_eq!(cpu.register(Register32::Esi), 0x0040_2187);
    assert_eq!(cpu.register(Register32::Edi), 0x0040_21c7);
    assert_eq!(cpu.eflags, 0xcad7);
    let mut values = [[0; 7]; 2];
    whole.memory.read(0x0040_21c0, &mut values[0]).unwrap();
    stepped.memory.read(0x0040_21c0, &mut values[1]).unwrap();
    assert_eq!(values, [[1, 2, 3, 4, 5, 6, 7]; 2]);
}
