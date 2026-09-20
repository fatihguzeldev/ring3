#[path = "support/crt_executable.rs"]
mod crt_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    LoadError, MemoryError, Process32, ProcessStop, Register32, StopReason,
};

fn read(process: &Process32, address: u64) -> u32 {
    let mut value = [0; 4];
    process.memory.read(address, &mut value).unwrap();
    u32::from_le_bytes(value)
}

#[test]
fn cdecl_keeps_arguments_for_the_caller_and_updates_process_state() {
    for application_type in [0_u32, 1, 2, u32::MAX] {
        let mut process = Process32::load(&crt_executable::pe32(application_type), 32).unwrap();
        let initial_stack = process.cpu.register(Register32::Esp);
        assert_eq!(process.crt_application_type(), 0);
        let result = process.run(4);
        assert_eq!((result.instructions, result.api_calls), (3, 1));
        assert_eq!(
            process.crt_application_type(),
            application_type.cast_signed()
        );
        assert_eq!(process.cpu.register(Register32::Eax), 0x1234);
        assert_eq!(process.cpu.register(Register32::Esp), initial_stack - 4);
        assert_eq!(
            read(&process, u64::from(initial_stack - 4)),
            application_type
        );
        assert_eq!(
            process.run(100).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(process.cpu.register(Register32::Ecx), application_type);
        assert_eq!(process.cpu.register(Register32::Esp), initial_stack);
        assert_eq!(process.cpu.register(Register32::Edx), 7);
        assert_eq!(process.cpu.register(Register32::Ebx), 0);
        assert_eq!(read(&process, 0x7000_2000), 35);
        assert_eq!(read(&process, 0x7000_2004), 7);
    }
}

#[test]
fn pointer_accessors_and_data_imports_share_writable_guest_storage() {
    let code = [
        0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6, 0xc7, 0, 25, 0, 0, 0, 0x8b, 0x1d, 0x64, 0x20,
        0x40, 0, 0x8b, 0x13, 0xff, 0x15, 0x68, 0x20, 0x40, 0, 0xc7, 0, 17, 0, 0, 0, 0x8b, 0x3d,
        0x6c, 0x20, 0x40, 0, 0x8b, 0x0f, 0xcc,
    ];
    let bytes = imported_executable::pe32(
        &code,
        "MSVCRT.dll",
        &["__p__fmode", "_fmode", "__p__commode", "_commode"],
    );
    let mut process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(
        process.run(100).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(
        process.cpu.register(Register32::Esi),
        process.cpu.register(Register32::Ebx)
    );
    assert_eq!(
        process.cpu.register(Register32::Eax),
        process.cpu.register(Register32::Edi)
    );
    assert_ne!(
        process.cpu.register(Register32::Esi),
        process.cpu.register(Register32::Edi)
    );
    assert_eq!(process.cpu.register(Register32::Edx), 25);
    assert_eq!(process.cpu.register(Register32::Ecx), 17);
    let fresh = Process32::load(&bytes, 32).unwrap();
    assert_eq!(read(&fresh, 0x7000_2000), 0x4000);
    assert_eq!(read(&fresh, 0x7000_2004), 0);
    assert_eq!(fresh.crt_application_type(), 0);
}

#[test]
fn crt_frame_faults_and_mapping_failures_are_explicit() {
    let bytes = crt_executable::pe32(2);
    let mut process = Process32::load(&bytes, 32).unwrap();
    process.run(3);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(result.api_calls, 0);
    assert_eq!(process.crt_application_type(), 0);
    assert_eq!(process.cpu, before);
    assert!(process.memory.fetch(0x7000_2000, &mut [0]).is_err());
    assert!(matches!(
        Process32::load(&bytes, 23),
        Err(LoadError::Memory(MemoryError::PageLimitExceeded))
    ));
    assert!(Process32::load(&bytes, 24).is_ok());
    let mut collision = bytes;
    collision[0xb4..0xb8].copy_from_slice(&0x7000_0000_u32.to_le_bytes());
    assert!(Process32::load(&collision, 32).is_err());
}
