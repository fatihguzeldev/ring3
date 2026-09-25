use super::executable;

use super::complement_executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

#[test]
fn mixed_width_and_fs_program_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut image = load_pe32(&complement_executable::pe32(), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(0x0040_2000);
        cpu.eflags = 0xced7;
        let mut count = 0;
        loop {
            let result = cpu.run(&mut image.memory, budget);
            count += result.instructions;
            if result.reason != StopReason::InstructionLimit {
                assert_eq!(result.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 50);
        }
        assert_eq!(count, 8);
        assert_eq!(cpu.register(Register32::Eax), 0x1234_5687);
        assert_eq!(cpu.register(Register32::Ebx), 0xffff_ffee);
        assert_eq!(cpu.register(Register32::Esi), 0x7fff_ffff);
        assert_eq!(cpu.eflags, 0xced7);
    }
}

fn encoding(bits: u32, operand: u8) -> Vec<u8> {
    let mut code = if bits == 16 { vec![0x66] } else { vec![] };
    code.extend_from_slice(&[if bits == 8 { 0xf6 } else { 0xf7 }, operand]);
    code
}

#[test]
fn every_register_view_complements_only_its_bits_without_changing_any_flags() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let values: Vec<_> = if bits == 8 {
            (0..256).collect()
        } else {
            vec![
                0,
                1,
                0x7f,
                0x80,
                0xff,
                mask >> 1,
                1 << (bits - 1),
                mask - 1,
                mask,
            ]
        };
        for index in 0..8_u8 {
            let code = encoding(bits, 0xd0 + index);
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            let parent = REGISTERS[usize::from(if bits == 8 { index % 4 } else { index })];
            let shift = if bits == 8 && index >= 4 { 8 } else { 0 };
            for &value in &values {
                for flags in [0, u32::MAX] {
                    let mut cpu = Cpu32::new(image.entry_point);
                    for register in REGISTERS {
                        cpu.set_register(register, 0x89ab_cdef);
                    }
                    let initial = (0x89ab_cdef & !(mask << shift)) | (value << shift);
                    cpu.set_register(parent, initial);
                    cpu.eflags = flags;
                    let mut expected = cpu;
                    expected.set_register(
                        parent,
                        (initial & !(mask << shift)) | ((mask - value) << shift),
                    );
                    expected.eip += u32::try_from(code.len()).unwrap();
                    let result = cpu.run(&mut image.memory, 1);
                    assert_eq!(
                        (result.reason, result.instructions),
                        (StopReason::InstructionLimit, 1)
                    );
                    assert_eq!(cpu, expected, "{bits} {index} {value:x} {flags:x}");
                }
            }
        }
    }
}

#[test]
fn memory_widths_cross_pages_and_support_fs_and_final_guest32_endpoints() {
    for bits in [8, 16, 32] {
        let size = usize::try_from(bits / 8).unwrap();
        for address in [0x0040_2001_u32, 0x0040_2fff, u32::MAX - bits / 8 + 1] {
            for fs in [false, true] {
                let mut code = encoding(bits, 0x15);
                let base = if fs { 0x0040_2000_u32 } else { 0 };
                code.extend_from_slice(&address.wrapping_sub(base).to_le_bytes());
                if fs {
                    code.insert(0, 0x64);
                }
                let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
                image
                    .memory
                    .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                image
                    .memory
                    .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                image.memory.write(u64::from(address - 1), &[0x55]).unwrap();
                image
                    .memory
                    .write(u64::from(address), &vec![0xa5; size])
                    .unwrap();
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_fs_base(base);
                cpu.eflags = 0xced7;
                let mut expected = cpu;
                expected.eip += u32::try_from(code.len()).unwrap();
                assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
                assert_eq!(cpu.eip, image.entry_point);
                assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                assert_eq!(cpu, expected);
                let mut bytes = vec![0; size];
                image.memory.read(u64::from(address), &mut bytes).unwrap();
                assert_eq!(bytes, vec![0x5a; size]);
                let mut neighbor = [0];
                image
                    .memory
                    .read(u64::from(address - 1), &mut neighbor)
                    .unwrap();
                assert_eq!(neighbor, [0x55]);
                if let Some(after) = address.checked_add(u32::try_from(size).unwrap()) {
                    image.memory.read(u64::from(after), &mut neighbor).unwrap();
                    assert_eq!(neighbor, [0]);
                }
            }
        }
    }
}

#[test]
fn inaccessible_or_wrapping_operands_preserve_cpu_and_every_writable_byte() {
    for (address, permission) in [
        (0x0040_2000_u32, Permissions::READ),
        (0x0040_2000, Permissions::NONE),
        (0x0040_2ffe, Permissions::READ),
        (0xffff_fffe, Permissions::READ_WRITE),
    ] {
        let mut code = encoding(32, 0x15);
        code.extend_from_slice(&address.to_le_bytes());
        let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
        if address == 0xffff_fffe {
            image
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
        }
        image.memory.write(u64::from(address), &[0xa5; 2]).unwrap();
        if address == 0x0040_2ffe {
            image
                .memory
                .map_zeroed(0x0040_3000, PAGE_SIZE, permission)
                .unwrap();
        } else if address == 0x0040_2000 {
            image
                .memory
                .protect(0x0040_2000, PAGE_SIZE, permission)
                .unwrap();
        }
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.eflags = 0xced7;
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
        if address == 0x0040_2000 {
            image
                .memory
                .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
        }
        let mut bytes = [0; 2];
        image.memory.read(u64::from(address), &mut bytes).unwrap();
        assert_eq!(bytes, [0xa5; 2]);
    }
}

#[test]
fn unsupported_prefixes_and_address_forms_do_not_execute() {
    for code in [
        &[0xf0, 0xf7, 0x15, 0, 0x20, 0x40, 0][..],
        &[0xf3, 0xf7, 0xd0],
        &[0xf2, 0xf6, 0xd4],
        &[0x67, 0xf7, 0x10],
        &[0x2e, 0xf7, 0x15, 0, 0x20, 0x40, 0],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert_eq!(result.reason, StopReason::UnsupportedInstruction);
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}
