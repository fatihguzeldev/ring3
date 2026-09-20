#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Access, LoadError, MemoryError, Process32, ProcessStop, Register32, StopReason,
};

#[test]
fn diagnostic_calls_stop_with_identity_without_guessing_the_abi() {
    for ordinal in [false, true] {
        let mut bytes = imported_executable::pe32(
            &[0x6a, 42, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0xcc],
            "Missing.dll",
            &["Absent"],
        );
        if ordinal {
            bytes[1088..1092].copy_from_slice(&0x8000_0017_u32.to_le_bytes());
        }
        assert!(matches!(
            Process32::load(&bytes, 32),
            Err(LoadError::UnresolvedImport { .. })
        ));
        let mut process = Process32::load_diagnostic(&bytes, 32).unwrap();
        let initial = process.cpu;
        assert_eq!(process.run(0).instructions, 0);
        assert_eq!(process.cpu, initial);
        assert_eq!(
            process.run(2).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        let before = process.cpu;
        let expected = ProcessStop::UnresolvedImport {
            address: 0x7100_0000,
            module: "Missing.dll".into(),
            symbol: if ordinal { "#23" } else { "Absent" }.into(),
        };
        for _ in 0..2 {
            let result = process.run(100);
            assert_eq!(result.reason, expected);
            assert_eq!((result.instructions, result.api_calls), (0, 0));
            assert_eq!(process.cpu, before);
        }
        assert_eq!(
            process.cpu.register(Register32::Esp),
            initial.register(Register32::Esp) - 8
        );
        assert_eq!(process.last_error().unwrap(), 0);
        assert_eq!(process.exit_code(), None);
        process.cpu.set_register(Register32::Esp, 0xffff_ffff);
        assert_eq!(process.run(1).reason, expected);
    }
}

#[test]
fn diagnostic_data_reads_fault_and_do_not_receive_fake_values() {
    let bytes = imported_executable::pe32(
        &[0xa1, 0x60, 0x20, 0x40, 0, 0x8b, 0x00, 0xcc],
        "Missing.dll",
        &["Data"],
    );
    let mut process = Process32::load_diagnostic(&bytes, 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.instructions, 1);
    assert_eq!(result.api_calls, 0);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
            address: 0x7100_0000,
            access: Access::Read
        }))
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0x7100_0000);
    assert_eq!(process.cpu.eip, 0x0040_1005);
}

#[test]
fn trap_addresses_keep_distinct_identities_and_padding_is_not_callable() {
    let bytes = imported_executable::pe32(&[0xcc], "Missing.dll", &["First", "Second"]);
    let mut process = Process32::load_diagnostic(&bytes, 32).unwrap();
    process.cpu.eip = 0x7100_0004;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnresolvedImport {
            address: 0x7100_0004,
            module: "Missing.dll".into(),
            symbol: "Second".into(),
        }
    );
    for address in [0x7100_0001, 0x7100_0008, 0x7100_0ffc] {
        process.cpu.eip = address;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(MemoryError::PermissionDenied {
                address: u64::from(address),
                access: Access::Execute
            }))
        );
    }
}

#[test]
fn diagnostics_respect_page_caps_collisions_and_known_apis() {
    let bytes = imported_executable::pe32(&[0xcc], "Missing.dll", &["Absent"]);
    assert!(matches!(
        Process32::load_diagnostic(&bytes, 23),
        Err(LoadError::Memory(MemoryError::PageLimitExceeded))
    ));
    assert!(Process32::load_diagnostic(&bytes, 24).is_ok());
    let mut collision = bytes.clone();
    collision[0xb4..0xb8].copy_from_slice(&0x7100_0000_u32.to_le_bytes());
    assert!(matches!(
        Process32::load_diagnostic(&collision, 32),
        Err(LoadError::Memory(MemoryError::AlreadyMapped { .. }))
    ));
    let known = imported_executable::pe32(
        &[0x6a, 42, 0xff, 0x15, 0x60, 0x20, 0x40, 0],
        "KERNEL32.dll",
        &["ExitProcess", "Missing"],
    );
    let mut process = Process32::load_diagnostic(&known, 32).unwrap();
    let result = process.run(100);
    assert_eq!(result.reason, ProcessStop::Exited(42));
    assert_eq!((result.instructions, result.api_calls), (2, 1));
}
