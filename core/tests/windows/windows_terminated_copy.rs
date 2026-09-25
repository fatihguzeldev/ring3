use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_00f0;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DEST: u32 = 0x0040_2280;

fn load() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["lstrcpyA"]),
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
fn copies_through_nul_without_padding_and_uses_two_argument_frame() {
    let mut p = load();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for source in [b"a\x80\0".as_slice(), b"\0"] {
        p.memory.write(u64::from(SOURCE), source).unwrap();
        p.memory.write(u64::from(DEST), &[0x55; 8]).unwrap();
        let before = prepare(&mut p, DEST, SOURCE);
        success(&mut p, before, DEST, 0x0040_1000);
        let mut output = [0; 8];
        p.memory.read(u64::from(DEST), &mut output).unwrap();
        assert_eq!(&output[..source.len()], source);
        assert!(output[source.len()..].iter().all(|&byte| byte == 0x55));
        assert_eq!(p.last_error().unwrap(), 77);
    }
    p.memory.write(u64::from(SOURCE), b"ABC\0").unwrap();
    let before = prepare(&mut p, STACK, SOURCE);
    success(&mut p, before, STACK, 0x0043_4241);
    prepare(&mut p, DEST, SOURCE);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn exact_page_and_u32_boundaries_do_not_read_past_nul() {
    let mut p = load();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, DEST, u32::MAX);
    success(&mut p, before, DEST, 0x0040_1000);
    let before = prepare(&mut p, u32::MAX, DEST);
    success(&mut p, before, u32::MAX, 0x0040_1000);
    p.memory.write(0x0040_2ffc, b"abc\0").unwrap();
    let before = prepare(&mut p, DEST, 0x0040_2ffc);
    success(&mut p, before, DEST, 0x0040_1000);
    p.memory.write(u64::from(SOURCE), b"abc\0").unwrap();
    let before = prepare(&mut p, 0x0040_2ffc, SOURCE);
    success(&mut p, before, 0x0040_2ffc, 0x0040_1000);
}

#[test]
fn access_errors_return_zero_and_protected_error_cell_is_atomic() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b"abc\0").unwrap();
    p.memory.write(0x0040_2ffe, b"xy").unwrap();
    for (dest, src) in [
        (DEST, 0),
        (0, SOURCE),
        (0x0040_2ffe, SOURCE),
        (DEST, 0x0040_2ffe),
    ] {
        let before = prepare(&mut p, dest, src);
        success(&mut p, before, 0, 0x0040_1000);
        assert_eq!(p.last_error().unwrap(), 87);
        let mut tail = [0; 2];
        p.memory.read(0x0040_2ffe, &mut tail).unwrap();
        assert_eq!(&tail, b"xy");
    }
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, 0x0040_2ffe, SOURCE);
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn overlap_and_missing_nul_at_scan_limit_are_explicitly_unsupported() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b"abc\0").unwrap();
    for destination in [SOURCE - 1, SOURCE, SOURCE + 1] {
        let before = prepare(&mut p, destination, SOURCE);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0x3000_0000, PAGE_SIZE * 16, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'x'; 65536]).unwrap();
    let before = prepare(&mut p, 0x3000_0000, 0x2000_0000);
    let result = p.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory.write(0x2000_ffff, &[0]).unwrap();
    let before = prepare(&mut p, 0x3000_0000, 0x2000_0000);
    success(&mut p, before, 0x3000_0000, 0x0040_1000);
    let mut tail = [0; 2];
    p.memory.read(0x3000_fffe, &mut tail).unwrap();
    assert_eq!(tail, [b'x', 0]);
}
