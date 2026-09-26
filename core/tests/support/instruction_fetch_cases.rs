use ring3_core::execution::{
    Access, Cpu32, GuestMemory, MemoryError, PAGE_SIZE, Permissions, Register32, StopReason,
};

pub fn complete_instructions_respect_page_and_address_limits() {
    for start in [0x1fff_u32, u32::MAX] {
        let page = u64::from(start) & !(PAGE_SIZE - 1);
        let mut memory = GuestMemory::new(2);
        memory
            .map_zeroed(page, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory.write(u64::from(start), &[0xcc]).unwrap();
        memory
            .protect(page, PAGE_SIZE, Permissions::READ_EXECUTE)
            .unwrap();
        if start != u32::MAX {
            memory
                .map_zeroed(page + PAGE_SIZE, PAGE_SIZE, Permissions::NONE)
                .unwrap();
        }
        let mut cpu = Cpu32::new(start);
        let result = cpu.run(&mut memory, 1);
        assert_eq!(result.reason, StopReason::Breakpoint);
        assert_eq!(result.instructions, 1);
        assert_eq!(cpu.eip, start.wrapping_add(1));
    }

    let mut memory = GuestMemory::new(2);
    memory
        .map_zeroed(0x1000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(0x1ffe, &[0xb8, 0x78, 0x56, 0x34, 0x12])
        .unwrap();
    memory
        .protect(0x1000, 2 * PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1ffe);
    assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::InstructionLimit);
    assert_eq!(cpu.register(Register32::Eax), 0x1234_5678);
    assert_eq!(cpu.eip, 0x2003);
}

pub fn failed_fetches_preserve_fault_order_and_cpu_state() {
    for (start, code, next, expected) in [
        (
            0x1fff,
            vec![0xb8],
            None,
            StopReason::MemoryFault(MemoryError::Unmapped { address: 0x2000 }),
        ),
        (
            0x1ffe,
            vec![0xb8, 0x78],
            Some(Permissions::READ_WRITE),
            StopReason::MemoryFault(MemoryError::PermissionDenied {
                address: 0x2000,
                access: Access::Execute,
            }),
        ),
        (
            0x1ffe,
            vec![0xf0, 0x90],
            Some(Permissions::NONE),
            StopReason::InvalidInstruction,
        ),
        (0x1ff1, vec![0x66; 15], None, StopReason::InvalidInstruction),
        (
            0x1ff2,
            vec![0x66; 14],
            None,
            StopReason::MemoryFault(MemoryError::Unmapped { address: 0x2000 }),
        ),
        (
            u32::MAX,
            vec![0xb8],
            None,
            StopReason::MemoryFault(MemoryError::AddressOverflow),
        ),
    ] {
        let page = u64::from(start) & !(PAGE_SIZE - 1);
        let mut memory = GuestMemory::new(2);
        memory
            .map_zeroed(page, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory.write(u64::from(start), &code).unwrap();
        memory
            .protect(page, PAGE_SIZE, Permissions::READ_EXECUTE)
            .unwrap();
        if let Some(permissions) = next {
            memory
                .map_zeroed(page + PAGE_SIZE, PAGE_SIZE, permissions)
                .unwrap();
        }
        let mut cpu = Cpu32::new(start);
        cpu.set_register(Register32::Eax, 0xabcd_ef01);
        cpu.eflags = 0x8d7;
        let before = cpu;
        let result = cpu.run(&mut memory, 1);
        assert_eq!(result.reason, expected);
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}
