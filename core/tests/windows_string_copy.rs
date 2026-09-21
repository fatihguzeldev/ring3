#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_00e4;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DEST: u32 = 0x0040_2280;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["lstrcpynA"]),
        64,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, destination: u32, source: u32, count: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, destination, source, count]
        .into_iter()
        .enumerate()
    {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn success(process: &mut Process32, before: Cpu32, value: u32, target: u32) {
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let mut expected = before;
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 16);
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
}

#[test]
fn bounded_copy_terminates_without_padding_and_preserves_last_error() {
    let mut process = process();
    process
        .memory
        .write(u64::from(SOURCE), b"a\x80.bc\0z")
        .unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    for count in [0, 1, 2, 5, 6, 9, u32::MAX, 0x8000_0000] {
        process.memory.write(u64::from(DEST), &[0x55; 10]).unwrap();
        let before = prepare(&mut process, DEST, SOURCE, count);
        success(&mut process, before, DEST, 0x0040_1000);
        let mut expected = [0x55; 10];
        if count != 0 {
            let length = usize::try_from((count - 1).min(5)).unwrap();
            expected[..length].copy_from_slice(&b"a\x80.bc"[..length]);
            expected[length] = 0;
        }
        let mut bytes = [0; 10];
        process.memory.read(u64::from(DEST), &mut bytes).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(process.last_error().unwrap(), 77);
    }
    let before = prepare(&mut process, DEST, SOURCE + 5, 50);
    success(&mut process, before, DEST, 0x0040_1000);
    let mut byte = [99];
    process.memory.read(u64::from(DEST), &mut byte).unwrap();
    assert_eq!(byte, [0]);
}

#[test]
fn touches_only_required_source_and_destination_bytes() {
    let mut process = process();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for pointer in [0, u32::MAX, 0x7000_0000] {
        let before = prepare(&mut process, pointer, pointer, 0);
        success(&mut process, before, pointer, 0x0040_1000);
    }
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let before = prepare(&mut process, u32::MAX, 0, 1);
    success(&mut process, before, u32::MAX, 0x0040_1000);
    process.memory.write(0x0040_2ffe, b"ab").unwrap();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    process
        .memory
        .map_zeroed(
            0x2000_0000,
            PAGE_SIZE * 2,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        )
        .unwrap();
    let before = prepare(&mut process, 0x2000_0fff, 0x0040_2ffe, 3);
    success(&mut process, before, 0x2000_0fff, 0x0040_1000);
    process
        .memory
        .protect(0x2000_0000, PAGE_SIZE * 2, Permissions::READ)
        .unwrap();
    let mut bytes = [0; 3];
    process.memory.read(0x2000_0fff, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ab\0");
    let before = prepare(&mut process, STACK + 20, u32::MAX, 2);
    success(&mut process, before, STACK + 20, 0x0040_1000);
}

#[test]
fn copy_faults_return_null_and_error_without_partial_output() {
    for (destination, source, count) in [
        (DEST, 0x0040_2ffe, 4),
        (0x0040_2ffe, SOURCE, 4),
        (u32::MAX, SOURCE, 4),
        (DEST, 0, 2),
        (0, SOURCE, 1),
    ] {
        let mut process = process();
        process.memory.write(u64::from(SOURCE), b"abc\0").unwrap();
        process.memory.write(0x0040_2ffe, b"!!").unwrap();
        process.memory.write(u64::from(DEST), b"!!!!").unwrap();
        let before = prepare(&mut process, destination, source, count);
        success(&mut process, before, 0, 0x0040_1000);
        assert_eq!(process.last_error().unwrap(), 87);
        let mut bytes = [0; 4];
        process.memory.read(u64::from(DEST), &mut bytes).unwrap();
        assert_eq!(&bytes, b"!!!!");
        process.memory.read(0x0040_2ffe, &mut bytes[..2]).unwrap();
        assert_eq!(&bytes[..2], b"!!");
    }
    let mut process = process();
    process
        .memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    prepare(&mut process, DEST, 0, 2);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
}

#[test]
fn overlapping_spans_and_excessive_actual_copy_are_unsupported() {
    let mut process = process();
    process
        .memory
        .write(u64::from(SOURCE), b"abcdef\0")
        .unwrap();
    for destination in [SOURCE, SOURCE + 1, SOURCE - 1] {
        let before = prepare(&mut process, destination, SOURCE, 5);
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    process
        .memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .map_zeroed(0x3000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    process
        .memory
        .write(0x2000_0000, &vec![b'x'; 65536])
        .unwrap();
    let before = prepare(&mut process, 0x3000_0000, 0x2000_0000, 65537);
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    let before = prepare(&mut process, 0x3000_0000, 0x2000_0000, 65536);
    success(&mut process, before, 0x3000_0000, 0x0040_1000);
    let mut tail = [0; 2];
    process.memory.read(0x3000_fffe, &mut tail).unwrap();
    assert_eq!(tail, [b'x', 0]);
    process.memory.write(0x2000_ffff, &[0]).unwrap();
    let before = prepare(&mut process, 0x3000_0000, 0x2000_0000, u32::MAX);
    success(&mut process, before, 0x3000_0000, 0x0040_1000);
}

#[test]
fn copy_can_overwrite_arguments_and_return_but_needs_a_complete_frame() {
    let mut process = process();
    process.memory.write(u64::from(SOURCE), b"ABC\0").unwrap();
    let before = prepare(&mut process, STACK, SOURCE, 4);
    success(&mut process, before, STACK, 0x0043_4241);
    prepare(&mut process, DEST, SOURCE, 4);
    process.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
}
