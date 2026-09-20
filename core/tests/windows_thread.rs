#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/thread_executable.rs"]
mod thread_executable;

use ring3_core::execution::{
    LoadError, MemoryError, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

fn read(process: &Process32, address: u64) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(address, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn guest_links_and_restores_its_exception_chain_and_shares_last_error() {
    for value in [42_u32, 0, u32::MAX] {
        let mut process = Process32::load(&thread_executable::pe32(value), 32).unwrap();
        assert_eq!(process.cpu.fs_base(), 0x7ffd_e000);
        assert_eq!(read(&process, 0x7ffd_e000), u32::MAX);
        assert_eq!(process.run(4).instructions, 4);
        let head = read(&process, 0x7ffd_e000);
        assert_eq!(head, process.cpu.register(Register32::Esp));
        assert_eq!(read(&process, u64::from(head)), u32::MAX);
        assert_eq!(read(&process, u64::from(head) + 4), 0x0040_1000);
        let result = process.run(100);
        assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
        assert_eq!(result.api_calls, 2);
        assert_eq!(read(&process, 0x7ffd_e000), u32::MAX);
        assert_eq!(process.cpu.register(Register32::Esi), 0x7ffd_e000);
        assert_eq!(process.cpu.register(Register32::Ecx), 0x1001_0000);
        assert_eq!(process.cpu.register(Register32::Edx), 0x1000_0000);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(process.cpu.register(Register32::Ebx), value);
        assert_eq!(process.cpu.register(Register32::Eax), value.wrapping_add(7));
        assert_eq!(process.last_error().unwrap(), value.wrapping_add(7));
    }
}

#[test]
fn thread_mapping_counts_toward_budget_and_refuses_collisions_and_execution() {
    let bytes = thread_executable::pe32(42);
    assert!(matches!(
        Process32::load(&bytes, 23),
        Err(LoadError::Memory(MemoryError::PageLimitExceeded))
    ));
    let mut process = Process32::load(&bytes, 24).unwrap();
    assert!(process.memory.fetch(0x7ffd_e000, &mut [0]).is_err());
    let mut collision = bytes;
    collision[0xb4..0xb8].copy_from_slice(&0x7ffd_0000_u32.to_le_bytes());
    collision[0xd0..0xd4].copy_from_slice(&0x1_0000_u32.to_le_bytes());
    assert!(matches!(
        Process32::load(&collision, 40),
        Err(LoadError::Memory(MemoryError::AlreadyMapped { .. }))
    ));
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    assert!(process.last_error().is_err());
}

#[test]
fn last_error_api_permission_faults_preserve_call_state() {
    for (api, permissions) in [
        ("SetLastError", Permissions::READ),
        ("GetLastError", Permissions::NONE),
    ] {
        let bytes = imported_executable::pe32(
            &[0x6a, 42, 0xff, 0x15, 0x60, 0x20, 0x40, 0],
            "KERNEL32.dll",
            &[api],
        );
        let mut process = Process32::load(&bytes, 32).unwrap();
        process.run(2);
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, permissions)
            .unwrap();
        let before = process.cpu;
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        process
            .memory
            .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(process.last_error().unwrap(), 0);
    }
}
