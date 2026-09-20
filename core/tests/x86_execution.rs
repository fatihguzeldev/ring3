#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{
    Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32,
};

#[test]
fn executes_guest_arithmetic_from_loaded_pe_bytes() {
    for (left, right) in [(7_u32, 35_u32), (123, 456), (u32::MAX, 1), (0x7fff_ffff, 1)] {
        let mut code = vec![0xb8];
        code.extend_from_slice(&left.to_le_bytes());
        code.push(0x05);
        code.extend_from_slice(&right.to_le_bytes());
        code.push(0xcc);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let result = cpu.run(&mut image.memory, 20);
        assert_eq!(result.reason, StopReason::Breakpoint);
        assert_eq!(result.instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), left.wrapping_add(right));
        assert_eq!(cpu.eip, image.entry_point + 11);
        assert_eq!(cpu.eflags & 1 != 0, left.checked_add(right).is_none());
        assert_eq!(cpu.eflags & 0x40 != 0, left.wrapping_add(right) == 0);
        assert_eq!(cpu.eflags & 0x800 != 0, left == 0x7fff_ffff);
    }
}

#[test]
fn branch_loop_consumes_budget_and_can_resume() {
    let code = [0xb9, 3, 0, 0, 0, 0x83, 0xe9, 1, 0x75, 0xfb, 0xcc];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 0).reason,
        StopReason::InstructionLimit
    );
    assert_eq!(cpu.eip, image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 3).reason,
        StopReason::InstructionLimit
    );
    assert_eq!(cpu.register(Register32::Ecx), 2);
    let result = cpu.run(&mut image.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 5);
    assert_eq!(cpu.register(Register32::Ecx), 0);
    let mut looping = load_pe32(&executable::pe32(&[0xeb, 0xfe]), 3).unwrap();
    let mut cpu = Cpu32::new(looping.entry_point);
    assert_eq!(cpu.run(&mut looping.memory, 100).instructions, 100);
    assert_eq!(cpu.eip, looping.entry_point);
}

#[test]
fn unsupported_and_invalid_instructions_preserve_faulting_state() {
    for code in [&[0x0f, 0xa2][..], &[0x66, 0xb8, 1, 0], &[0xf0, 0x90]] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 55);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 5);
        assert!(matches!(
            result.reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn decoding_does_not_fetch_beyond_the_instruction() {
    let mut memory = GuestMemory::new(1);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x1fff, &[0xcc]).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1fff);
    assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::Breakpoint);
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu.eip, 0x2000);
}

#[test]
fn instruction_bytes_cannot_wrap_across_the_32_bit_code_limit() {
    let mut memory = GuestMemory::new(2);
    for address in [0, 0xffff_f000] {
        memory
            .map_zeroed(address, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
    }
    memory.write(0xffff_ffff, &[0xb8]).unwrap();
    memory.write(0, &[42, 0, 0, 0]).unwrap();
    for address in [0, 0xffff_f000] {
        memory
            .protect(address, PAGE_SIZE, Permissions::READ_EXECUTE)
            .unwrap();
    }
    let mut cpu = Cpu32::new(u32::MAX);
    cpu.set_register(Register32::Eax, 55);
    let before = cpu;
    let result = cpu.run(&mut memory, 1);
    assert_eq!(
        result.reason,
        StopReason::MemoryFault(ring3_core::execution::MemoryError::AddressOverflow)
    );
    assert_eq!(result.instructions, 0);
    assert_eq!(cpu, before);
}

#[test]
fn sign_extended_immediates_and_compare_update_flags_without_clobbering_values() {
    for (code, expected, flags) in [
        (&[0xb8, 1, 0, 0, 0, 0x83, 0xc0, 0xff, 0xcc][..], 0, 0x55),
        (&[0xb8, 0, 0, 0, 0, 0x83, 0xe8, 1, 0xcc][..], u32::MAX, 0x95),
        (
            &[
                0xb8, 0, 0, 0, 0, 0x83, 0xe8, 1, 0x83, 0xf8, 0xff, 0x74, 5, 0xb8, 42, 0, 0, 0, 0xcc,
            ][..],
            u32::MAX,
            0x44,
        ),
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        assert_eq!(
            cpu.run(&mut image.memory, 10).reason,
            StopReason::Breakpoint
        );
        assert_eq!(cpu.register(Register32::Eax), expected);
        assert_eq!(cpu.eflags & 0x8d5, flags);
    }
}
