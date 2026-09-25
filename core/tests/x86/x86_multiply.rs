use super::executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

#[test]
fn unsigned_dword_multiply_writes_both_halves_and_only_defined_flags() {
    for (eax, ecx, low, high) in [
        (3_u32, 4_u32, 12_u32, 0_u32),
        (0xaaaa_aaab, 6, 2, 4),
        (u32::MAX, u32::MAX, 1, 0xffff_fffe),
    ] {
        let mut image = load_pe32(&executable::pe32(&[0xf7, 0xe1]), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, eax);
        cpu.set_register(Register32::Ecx, ecx);
        cpu.set_register(Register32::Edx, 0x1234_5678);
        cpu.eflags = 0xced7;
        let before = cpu;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::InstructionLimit
        );
        assert_eq!(cpu.eip, before.eip + 2);
        assert_eq!(cpu.register(Register32::Eax), low);
        assert_eq!(cpu.register(Register32::Edx), high);
        assert_eq!(cpu.register(Register32::Ecx), ecx);
        assert_eq!(cpu.eflags & !0x801, before.eflags & !0x801);
        assert_eq!(cpu.eflags & 0x801, if high == 0 { 0 } else { 0x801 });
    }
}

#[test]
fn unsigned_dword_multiply_reads_aliases_before_writing_destinations() {
    for (source, eax, edx, low, high) in [
        (0xe0_u8, 0x10000_u32, 7_u32, 0, 1),
        (0xe2, 0x8000_0000, 3, 0x8000_0000, 1),
    ] {
        let mut image = load_pe32(&executable::pe32(&[0xf7, source]), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, eax);
        cpu.set_register(Register32::Edx, edx);
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::InstructionLimit
        );
        assert_eq!(cpu.register(Register32::Eax), low);
        assert_eq!(cpu.register(Register32::Edx), high);
    }
}

#[test]
fn unsigned_dword_multiply_memory_source_and_faults_are_atomic() {
    for address in [0x0040_2200_u32, 0x6000_0000, 0x0040_2fff] {
        let mut code = vec![0xf7, 0x25];
        code.extend(address.to_le_bytes());
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 0x8000_0000);
        cpu.set_register(Register32::Edx, 0x1234_5678);
        cpu.eflags = 0xced7;
        if address == 0x0040_2200 {
            image
                .memory
                .write(u64::from(address), &3_u32.to_le_bytes())
                .unwrap();
        }
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        if address == 0x0040_2200 {
            assert_eq!(result.reason, StopReason::InstructionLimit);
            assert_eq!(cpu.register(Register32::Eax), 0x8000_0000);
            assert_eq!(cpu.register(Register32::Edx), 1);
        } else {
            assert!(matches!(result.reason, StopReason::MemoryFault(_)));
            assert_eq!(cpu, before);
        }
        if address == 0x0040_2fff {
            image
                .memory
                .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
                .unwrap();
            image
                .memory
                .write(u64::from(address), &3_u32.to_le_bytes())
                .unwrap();
            assert_eq!(
                cpu.run(&mut image.memory, 1).reason,
                StopReason::InstructionLimit
            );
            assert_eq!(cpu.register(Register32::Edx), 1);
        }
    }
}

#[test]
fn other_unsigned_multiply_width_remains_outside_this_slice() {
    let mut image = load_pe32(&executable::pe32(&[0x66, 0xf7, 0xe1]), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
