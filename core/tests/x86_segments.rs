#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{
    Cpu32, GuestMemory, MemoryError, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32,
};

#[test]
fn fs_memory_uses_segment_base_while_lea_returns_the_effective_offset() {
    let code = [
        0xb8, 42, 0, 0, 0, 0x64, 0xa3, 0, 0, 0, 0, 0xbb, 4, 0, 0, 0, 0xb9, 3, 0, 0, 0, 0x64, 0x89,
        0x44, 0x8b, 4, 0x64, 0x83, 0x44, 0x8b, 4, 7, 0x64, 0x8b, 0x54, 0x8b, 4, 0x64, 0x8d, 0x74,
        0x8b, 4, 0x64, 0xa1, 0, 0, 0, 0, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(cpu.fs_base(), 0);
    cpu.set_fs_base(0x0040_2000);
    assert_eq!(
        cpu.run(&mut image.memory, 100).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 42);
    assert_eq!(cpu.register(Register32::Edx), 49);
    assert_eq!(cpu.register(Register32::Esi), 20);
    assert_eq!(cpu.fs_base(), 0x0040_2000);
}

#[test]
fn fs_push_reads_segmented_memory_but_writes_the_flat_stack() {
    let code = [0x64, 0xff, 0x35, 0, 0, 0, 0, 0x64, 0x58, 0xcc];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.set_register(Register32::Esp, 0x0040_3000);
    assert_eq!(
        cpu.run(&mut image.memory, 10).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu.register(Register32::Eax), 17);
    assert_eq!(cpu.register(Register32::Esp), 0x0040_3000);
}

#[test]
fn fs_faults_and_unsupported_segments_leave_faulting_state_unchanged() {
    for (code, base) in [
        (&[0x64, 0xa3, 0, 0, 0, 0][..], 0x0040_1000),
        (&[0x64, 0xa1, 0, 0, 0, 0][..], 0x8000_0000),
        (&[0x65, 0xa1, 0, 0, 0, 0][..], 0x0040_2000),
        (&[0x64, 0x67, 0xa1, 0, 0][..], 0x0040_2000),
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(base);
        cpu.set_register(Register32::Eax, 42);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(
            result.reason,
            StopReason::MemoryFault(_) | StopReason::UnsupportedInstruction
        ));
        assert_eq!(result.instructions, 0);
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
fn fs_linear_addresses_wrap_but_dword_spans_cannot_cross_four_gib() {
    let mut memory = GuestMemory::new(2);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory
        .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0, &42_u32.to_le_bytes()).unwrap();
    memory.write(0x1000, &[0x64, 0xa1, 0x10, 0, 0, 0]).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    cpu.set_fs_base(0xffff_fff0);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), 42);
    cpu.eip = 0x1000;
    cpu.set_fs_base(0xffff_ffee);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(MemoryError::AddressOverflow)
    );
    assert_eq!(cpu, before);
}
