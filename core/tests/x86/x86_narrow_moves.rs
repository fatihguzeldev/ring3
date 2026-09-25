use super::executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

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
fn every_byte_and_word_register_preserves_its_parent_bits_and_flags() {
    for index in 0..8_u8 {
        for word in [false, true] {
            let code = if word {
                vec![0x66, 0xb8 + index, 0x34, 0x12]
            } else {
                vec![0xb0 + index, 0x5a]
            };
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            for register in REGISTERS {
                cpu.set_register(register, 0x89ab_cdef);
            }
            cpu.eflags = 0xced7;
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            let parent = usize::from(if word { index } else { index % 4 });
            let expected = if word {
                0x89ab_1234
            } else if index < 4 {
                0x89ab_cd5a
            } else {
                0x89ab_5aef
            };
            for (i, register) in REGISTERS.iter().enumerate() {
                assert_eq!(
                    cpu.register(*register),
                    if i == parent { expected } else { 0x89ab_cdef }
                );
            }
            assert_eq!(cpu.eflags, 0xced7);
        }
    }
}

#[test]
fn aliased_register_sources_are_read_before_partial_writeback() {
    for (code, expected) in [
        (&[0x88, 0xe0][..], 0x1234_abab),
        (&[0x88, 0xc4][..], 0x1234_cdcd),
        (&[0x66, 0x89, 0xd8][..], 0x1234_4321),
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 0x1234_abcd);
        cpu.set_register(Register32::Ebx, 0x8765_4321);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu.register(Register32::Eax), expected);
    }
}

#[test]
fn narrow_memory_moves_touch_only_their_width_at_the_end_of_a_page() {
    for word in [false, true] {
        for absolute in [false, true] {
            let address = if word { 0x0040_2ffe_u32 } else { 0x0040_2fff };
            let mut code = if word { vec![0x66] } else { vec![] };
            if absolute {
                code.push(if word { 0xa3 } else { 0xa2 });
            } else {
                code.extend_from_slice(&[if word { 0x89 } else { 0x88 }, 0x05]);
            }
            code.extend_from_slice(&address.to_le_bytes());
            if word {
                code.push(0x66);
            }
            if absolute {
                code.push(if word { 0xa1 } else { 0xa0 });
            } else {
                code.extend_from_slice(&[if word { 0x8b } else { 0x8a }, 0x05]);
            }
            code.extend_from_slice(&address.to_le_bytes());
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            image.memory.write(0x0040_2ffc, &[0xaa; 4]).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Eax, 0x1234_5678);
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            cpu.set_register(Register32::Eax, 0xabcd_ef00);
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            assert_eq!(
                cpu.register(Register32::Eax),
                if word { 0xabcd_5678 } else { 0xabcd_ef78 }
            );
            let mut data = [0; 4];
            image.memory.read(0x0040_2ffc, &mut data).unwrap();
            assert_eq!(
                data,
                if word {
                    [0xaa, 0xaa, 0x78, 0x56]
                } else {
                    [0xaa, 0xaa, 0xaa, 0x78]
                }
            );
        }
    }
}

#[test]
fn writes_fault_atomically_and_fs_addressing_stays_32_bit() {
    for address in [0x0040_2fff_u32, u32::MAX, 0x0040_2000] {
        let mut code = vec![0x66, 0xc7, 0x05];
        code.extend_from_slice(&address.to_le_bytes());
        code.extend_from_slice(&[0x34, 0x12]);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        if address == 0x0040_2000 {
            image
                .memory
                .protect(0x0040_2000, 4096, Permissions::READ)
                .unwrap();
        }
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
        let mut byte = [0];
        image.memory.read(0x0040_2fff, &mut byte).unwrap();
        assert_eq!(byte, [0]);
    }
    let code = [0x64, 0x66, 0xc7, 0x00, 0x34, 0x12, 0x64, 0x8a, 0x20];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
    assert_eq!(cpu.register(Register32::Eax), 0x3400);
    for code in [
        &[0x67, 0x8a, 0x00][..],
        &[0x67, 0x66, 0xa1, 0, 0x20][..],
        &[0x8c, 0xc0][..],
        &[0xf3, 0xb0, 1][..],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
}
