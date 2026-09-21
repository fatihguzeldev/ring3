#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/string_prefix_executable.rs"]
mod string_prefix_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0170;
const STACK: u32 = 0x1000_ff00;
const LEFT: u32 = 0x0040_2180;
const RIGHT: u32 = 0x0040_2190;

fn process() -> Process32 {
    Process32::load(&string_prefix_executable::pe32(), 64).unwrap()
}

fn prepare(p: &mut Process32, left: u32, right: u32, count: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, left, right, count].into_iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: i32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value.cast_unsigned());
    assert_eq!(p.cpu, expected);
}

#[test]
fn imported_prefix_comparison_runs_whole_or_stepwise() {
    for budget in [1, 40] {
        let mut p = process();
        let mut counts = (0, 0);
        loop {
            let run = p.run(budget);
            counts.0 += run.instructions;
            counts.1 += run.api_calls;
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 40);
        }
        assert_eq!(counts, (12, 2));
        assert_eq!(p.cpu.register(Register32::Esi), 0);
        assert_eq!(p.cpu.register(Register32::Eax), (-22_i32).cast_unsigned());
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn unsigned_first_difference_nul_and_count_preserve_inputs_and_error_state() {
    let mut p = process();
    p.memory.write(0x7000_2020, &88_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    for (left, right) in [
        (b"abcD\0X".as_slice(), b"abcZ\0Y".as_slice()),
        (b"abc\0D", b"abc\0Z"),
        (b"abc\0", b"abcZ"),
        (b"\xff\0", b"\x7f\0"),
        (b"\x80a\0", b"\xffa\0"),
    ] {
        p.memory.write(u64::from(LEFT), left).unwrap();
        p.memory.write(u64::from(RIGHT), right).unwrap();
        for count in 0..=left.len().min(right.len()) {
            let prefix = |bytes: &[u8]| {
                bytes
                    .iter()
                    .take(count)
                    .copied()
                    .take_while(|b| *b != 0)
                    .collect::<Vec<_>>()
            };
            let expected = prefix(left).cmp(&prefix(right));
            let difference = left
                .iter()
                .zip(right)
                .take(count)
                .find_map(|(&a, &b)| {
                    if a != b {
                        Some(i32::from(a) - i32::from(b))
                    } else if a == 0 {
                        Some(0)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            assert_eq!(difference.cmp(&0), expected);
            let before = prepare(&mut p, LEFT, RIGHT, u32::try_from(count).unwrap());
            assert_eq!(p.run(0).api_calls, 0);
            assert_eq!(p.cpu, before);
            success(&mut p, before, difference);
            let mut bytes = vec![0; left.len()];
            p.memory.read(u64::from(LEFT), &mut bytes).unwrap();
            assert_eq!(bytes, left);
        }
    }
    p.memory.write(u64::from(LEFT), b"aa\0").unwrap();
    let before = prepare(&mut p, LEFT, LEFT + 1, 2);
    success(&mut p, before, 97);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    let mut error = [0; 4];
    p.memory.read(0x7000_2020, &mut error).unwrap();
    assert_eq!(error, 88_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn zero_count_and_early_results_do_not_read_trailing_unmapped_or_overflowing_bytes() {
    let mut p = process();
    let before = prepare(&mut p, 0, u32::MAX, 0);
    success(&mut p, before, 0);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (a, b, count, difference) in [
        (0, 0, 2, 0),
        (128, 127, 2, 1),
        (42, 42, 1, 0),
        (0, 1, 65536, -1),
    ] {
        p.memory.write(0xffff_ffff, &[a]).unwrap();
        p.memory.write(0x0040_2fff, &[b]).unwrap();
        let before = prepare(&mut p, u32::MAX, 0x0040_2fff, count);
        success(&mut p, before, difference);
    }
    for (left, right, count) in [
        (0, RIGHT, 1),
        (LEFT, 0, 1),
        (0x0040_2fff, RIGHT, 2),
        (u32::MAX, RIGHT, 2),
    ] {
        p.memory.write(u64::from(RIGHT), b"xx").unwrap();
        p.memory.write(0xffff_ffff, b"x").unwrap();
        p.memory.write(0x0040_2fff, b"x").unwrap();
        let before = prepare(&mut p, left, right, count);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    p.memory.write(u64::from(LEFT), b"zz\0").unwrap();
    p.memory.write(u64::from(RIGHT), b"zz\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, LEFT, RIGHT, 3);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 0);
}

#[test]
fn scan_limit_and_cdecl_frame_checks_are_explicit() {
    let mut p = process();
    p.memory
        .map_zeroed(0x2000_0000, 65536, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'x'; 65536]).unwrap();
    let before = prepare(&mut p, 0x2000_0000, 0x2000_0000, 65536);
    success(&mut p, before, 0);
    for count in [65537, u32::MAX] {
        let before = prepare(&mut p, 0, 0, count);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    prepare(&mut p, 0, 0, 0);
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}
