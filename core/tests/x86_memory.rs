#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{
    Cpu32, GuestMemory, MemoryError, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32,
};

#[test]
fn memory_operands_and_lea_use_base_index_scale_and_displacement() {
    let code = [
        0xbb, 0, 0x20, 0x40, 0, 0xb9, 3, 0, 0, 0, 0xc7, 0x44, 0x8b, 4, 7, 0, 0, 0, 0x8b, 0x44,
        0x8b, 4, 0x83, 0xc0, 35, 0xa3, 0, 0x20, 0x40, 0, 0x8d, 0x54, 0x8b, 4, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 20).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 42);
    assert_eq!(cpu.register(Register32::Edx), 0x0040_2010);
    let mut value = [0; 4];
    image.memory.read(0x0040_2000, &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), 42);
}

#[test]
fn data_permission_faults_do_not_modify_registers_or_memory() {
    for code in [&[0xa3, 0, 0x10, 0x40, 0][..], &[0xa1, 0, 0, 0, 0x10]] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 42);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(cpu, before);
        let mut unchanged = vec![0; code.len()];
        image
            .memory
            .read(u64::from(image.entry_point), &mut unchanged)
            .unwrap();
        assert_eq!(unchanged, code);
    }
}

#[test]
fn a_dword_cannot_cross_the_32_bit_data_limit() {
    let mut memory = GuestMemory::new(3);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory
        .map_zeroed(0xffff_f000, 2 * PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(0x1000, &[0xa1, 0xfe, 0xff, 0xff, 0xff])
        .unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(MemoryError::AddressOverflow)
    );
    assert_eq!(cpu, before);
}

#[test]
fn register_xor_and_memory_arithmetic_execute_without_special_cases() {
    let code = [
        0x31, 0xc0, 0xbb, 0, 0x20, 0x40, 0, 0x83, 0x03, 25, 0x03, 0x03, 0x3b, 0x03, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, u32::MAX);
    assert_eq!(
        cpu.run(&mut image.memory, 20).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 42);
    assert_ne!(cpu.eflags & 0x40, 0);
}
