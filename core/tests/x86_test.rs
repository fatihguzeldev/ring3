#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

#[test]
fn documented_dword_forms_only_update_defined_flags() {
    for (left, right, flags) in [
        (0_u32, 0_u32, 0x44),
        (u32::MAX, 0, 0x44),
        (0x8000_0000, 0x8000_0000, 0x84),
        (0xff, 1, 0),
        (3, 3, 4),
        (0xf0, 0x0f, 0x44),
    ] {
        let mut accumulator = vec![0xa9];
        accumulator.extend_from_slice(&right.to_le_bytes());
        let mut register = vec![0xf7, 0xc0];
        register.extend_from_slice(&right.to_le_bytes());
        let mut memory = vec![0xf7, 0x05, 0, 0x20, 0x40, 0];
        memory.extend_from_slice(&right.to_le_bytes());
        for code in [
            accumulator,
            register,
            memory,
            vec![0x85, 0xd8],
            vec![0x85, 0x1d, 0, 0x20, 0x40, 0],
        ] {
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            image
                .memory
                .write(0x0040_2000, &left.to_le_bytes())
                .unwrap();
            image
                .memory
                .protect(0x0040_2000, 4096, Permissions::READ)
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Eax, left);
            cpu.set_register(Register32::Ebx, right);
            cpu.eflags = 0xced7;
            let before = cpu;
            assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
            assert_eq!(cpu, before);
            let result = cpu.run(&mut image.memory, 1);
            assert_eq!(result.reason, StopReason::InstructionLimit);
            assert_eq!(result.instructions, 1);
            assert_eq!(
                cpu.eip,
                image.entry_point + u32::try_from(code.len()).unwrap()
            );
            assert_eq!(cpu.register(Register32::Eax), left);
            assert_eq!(cpu.register(Register32::Ebx), right);
            assert_eq!(cpu.eflags & 0x8c5, flags);
            assert_eq!(cpu.eflags & !0x8d5, before.eflags & !0x8d5);
            let mut unchanged = [0; 4];
            image.memory.read(0x0040_2000, &mut unchanged).unwrap();
            assert_eq!(unchanged, left.to_le_bytes());
        }
    }
}

#[test]
fn failed_test_reads_preserve_cpu_state_including_flags() {
    for address in [0x0040_3000_u32, 0x0040_2fff, u32::MAX, 0x0040_2000] {
        let mut code = vec![0x85, 0x1d];
        code.extend_from_slice(&address.to_le_bytes());
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        if address == 0x0040_2000 {
            image
                .memory
                .protect(0x0040_2000, 4096, Permissions::NONE)
                .unwrap();
        }
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Ebx, u32::MAX);
        cpu.eflags = 0xced7;
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn fs_test_and_conditional_branch_resume_without_changing_operands() {
    let code = [0x64, 0x85, 0x18, 0x75, 1, 0x90, 0xcc];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.set_register(Register32::Ebx, 1);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), 0);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    assert_eq!(cpu.eip, image.entry_point + 6);
    assert_eq!(cpu.run(&mut image.memory, 1).reason, StopReason::Breakpoint);
}

#[test]
fn unsupported_widths_prefixes_and_undocumented_alias_do_not_execute() {
    for code in [
        &[0x84, 0xc0][..],
        &[0x66, 0x85, 0xc0][..],
        &[0xf7, 0xc8, 1, 0, 0, 0][..],
        &[0xf3, 0x85, 0xc0][..],
        &[0xf0, 0x85, 0xc0][..],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(
            result.reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(cpu, before);
    }
}
