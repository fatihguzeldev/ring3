#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_00f4;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DEST: u32 = 0x0040_2280;

fn load() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["lstrcatA"]),
        64,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, destination: u32, source: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, destination, source].into_iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: u32, target: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 12);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

#[test]
fn append_returns_original_pointer_and_preserves_prefix_and_tail() {
    let mut p = load();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for (prefix, source, expected) in [
        (
            b"ab\0".as_slice(),
            b"\x80z\0".as_slice(),
            b"ab\x80z\0".as_slice(),
        ),
        (b"ab\0", b"\0", b"ab\0"),
        (b"\0", b"xy\0", b"xy\0"),
    ] {
        p.memory.write(u64::from(DEST), &[0x55; 10]).unwrap();
        p.memory.write(u64::from(DEST), prefix).unwrap();
        p.memory.write(u64::from(SOURCE), source).unwrap();
        let before = prepare(&mut p, DEST, SOURCE);
        success(&mut p, before, DEST, 0x0040_1000);
        let mut output = [0; 10];
        p.memory.read(u64::from(DEST), &mut output).unwrap();
        assert_eq!(&output[..expected.len()], expected);
        assert!(output[expected.len()..].iter().all(|&byte| byte == 0x55));
        assert_eq!(p.last_error().unwrap(), 77);
    }
}

#[test]
fn readonly_prefix_and_last_address_need_only_actual_append_write_access() {
    let mut p = load();
    p.memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 2, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'a'; 4096]).unwrap();
    p.memory
        .protect(0x2000_0000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    p.memory.write(u64::from(SOURCE), b"b\0").unwrap();
    let before = prepare(&mut p, 0x2000_0000, SOURCE);
    success(&mut p, before, 0x2000_0000, 0x0040_1000);
    let mut boundary = [0; 3];
    p.memory.read(0x2000_0fff, &mut boundary).unwrap();
    assert_eq!(&boundary, b"ab\0");
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, u32::MAX, SOURCE + 1);
    success(&mut p, before, u32::MAX, 0x0040_1000);
}

#[test]
fn scans_and_output_fail_without_partial_copy_and_error_writes_are_checked() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b"xyz\0").unwrap();
    p.memory.write(0x0040_2ffd, b"ab\0").unwrap();
    for (destination, source) in [(0x0040_2ffd, SOURCE), (DEST, 0), (0, SOURCE)] {
        let before = prepare(&mut p, destination, source);
        success(&mut p, before, 0, 0x0040_1000);
        assert_eq!(p.last_error().unwrap(), 87);
        let mut bytes = [0; 3];
        p.memory.read(0x0040_2ffd, &mut bytes).unwrap();
        assert_eq!(&bytes, b"ab\0");
    }
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    let before = prepare(&mut p, u32::MAX, SOURCE);
    success(&mut p, before, 0, 0x0040_1000);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, 0x0040_2ffd, SOURCE);
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn destination_scan_bound_and_overlap_stop_explicitly() {
    let mut p = load();
    p.memory.write(u64::from(DEST), b"abc\0").unwrap();
    for source in [DEST, DEST + 1, DEST + 3] {
        let before = prepare(&mut p, DEST, source);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'a'; 65536]).unwrap();
    let before = prepare(&mut p, 0x2000_0000, SOURCE);
    let result = p.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory.write(0x2000_ffff, &[0]).unwrap();
    let before = prepare(&mut p, 0x2000_0000, SOURCE);
    success(&mut p, before, 0x2000_0000, 0x0040_1000);
}

#[test]
fn append_captures_arguments_and_rereads_overwritten_return() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b"ABC\0").unwrap();
    let before = prepare(&mut p, STACK, SOURCE);
    success(&mut p, before, STACK, 0x0043_4241);
    prepare(&mut p, DEST, SOURCE);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}
