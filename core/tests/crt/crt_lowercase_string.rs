use super::imported_executable;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01c0;
const BUFFER: u32 = 0x0040_2180;
const ERRNO: u32 = 0x7000_2020;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    let code = [
        0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 4, 0xcc,
    ];
    Process32::load(
        &imported_executable::pe32(&code, "MSVCRT.dll", &["_strlwr"]),
        128,
    )
    .unwrap()
}

fn bytes(process: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn call(process: &mut Process32, pointer: u32) -> ring3_core::execution::ProcessResult {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process
        .memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(STACK + 4), &pointer.to_le_bytes())
        .unwrap();
    process.run(1)
}

#[test]
fn imported_strlwr_changes_ascii_in_place_and_returns_the_same_pointer() {
    let mut process = process();
    let input = b"MiXeD-09_\xc4\0tail";
    process.memory.write(u64::from(BUFFER), input).unwrap();
    let result = process.run(20);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), BUFFER);
    assert_eq!(bytes(&process, BUFFER, input.len()), b"mixed-09_\xc4\0tail");
}

#[test]
fn strlwr_preflights_writes_and_handles_null_like_the_crt_string_family() {
    let mut process = process();
    process.memory.write(u64::from(BUFFER), b"UPPER\0").unwrap();
    process
        .memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    let before = process.cpu;
    let result = call(&mut process, BUFFER);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu.eip, API);
    assert_eq!(process.cpu.register(Register32::Esp), STACK);
    assert_eq!(bytes(&process, BUFFER, 6), b"UPPER\0");
    assert_eq!(process.cpu.eflags, before.eflags);

    process
        .memory
        .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
        .unwrap();
    let result = call(&mut process, 0);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!(process.cpu.register(Register32::Eax), 0);
    assert_eq!(
        u32::from_le_bytes(bytes(&process, ERRNO, 4).try_into().unwrap()),
        22
    );
}
