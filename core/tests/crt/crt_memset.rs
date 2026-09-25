use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0114;
const STACK: u32 = 0x1000_ff00;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["memset"]),
        32,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, address: u32, value: u32, count: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (i, word) in [0x0040_1000, address, value, count].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + i as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn success(process: &mut Process32, before: Cpu32, address: u32, target: u32) {
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let mut expected = before;
    expected.set_register(Register32::Eax, address);
    expected.set_register(Register32::Esp, STACK + 4);
    expected.eip = target;
    assert_eq!(process.cpu, expected);
}

#[test]
fn fills_exact_bytes_across_pages_without_read_permission() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0x0040_3000, PAGE_SIZE * 2, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0x0040_2ffd, &[9, 9, 9]).unwrap();
    process.memory.write(0x0040_4001, &[9, 9]).unwrap();
    process
        .memory
        .protect(
            0x0040_3000,
            PAGE_SIZE,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        )
        .unwrap();
    process
        .memory
        .write(0x7ffd_e034, &99_u32.to_le_bytes())
        .unwrap();
    let before = prepare(&mut process, 0x0040_2ffe, 0x1234_56a5, 4099);
    success(&mut process, before, 0x0040_2ffe, 0x0040_1000);
    assert_eq!(process.last_error().unwrap(), 99);
    process
        .memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let mut bytes = [0; 4101];
    process.memory.read(0x0040_2ffd, &mut bytes).unwrap();
    assert_eq!(bytes[0], 9);
    assert_eq!(bytes[4100], 9);
    assert_eq!(&bytes[1..4100], &[0xa5; 4099]);
}

#[test]
fn zero_length_returns_any_pointer_without_memory_or_last_error_access() {
    let mut process = process();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for address in [0, 0x7000_0000, u32::MAX] {
        let before = prepare(&mut process, address, u32::MAX, 0);
        success(&mut process, before, address, 0x0040_1000);
    }
}

#[test]
fn invalid_destination_and_frames_leave_the_entire_fill_untouched() {
    for (address, count, readonly) in [
        (0x0040_2ffe, 4, false),
        (0x0040_2ffe, 4, true),
        (0xffff_fffe, 4, false),
        (0x0040_2ffe, u32::MAX, false),
    ] {
        let mut process = process();
        if readonly {
            process
                .memory
                .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
                .unwrap();
        }
        if address == 0xffff_fffe {
            process
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
        }
        process.memory.write(u64::from(address), &[9; 2]).unwrap();
        let before = prepare(&mut process, address, 0, count);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        let mut bytes = [0; 2];
        process.memory.read(u64::from(address), &mut bytes).unwrap();
        assert_eq!(bytes, [9; 2]);
    }
    let mut process = process();
    prepare(&mut process, 0x0040_2180, 42, 4);
    process.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let before = process.cpu;
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(process.cpu, before);
    let mut bytes = [1; 4];
    process.memory.read(0x0040_2180, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4]);
}

#[test]
fn fill_can_alias_captured_arguments_and_saved_return() {
    let mut process = process();
    let before = prepare(&mut process, STACK, 0x12a, 16);
    success(&mut process, before, STACK, 0x2a2a_2a2a);
    let mut bytes = [0; 16];
    process.memory.read(u64::from(STACK), &mut bytes).unwrap();
    assert_eq!(bytes, [42; 16]);
    assert!(matches!(
        process.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
}
