use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0138;
const STACK: u32 = 0x1000_ff00;
const LEFT: u32 = 0x0040_2181;
const RIGHT: u32 = 0x0040_2801;

fn load() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["memcmp"]),
        64,
    )
    .unwrap()
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
fn unsigned_first_difference_matches_independent_slice_ordering() {
    let mut p = load();
    let mut left = [0; 600];
    for (i, value) in left.iter_mut().enumerate() {
        *value = u8::try_from((i * 37) % 256).unwrap();
    }
    for offset in [0, 1, 127, 255, 256, 257, 511, 599] {
        for (a, b) in [(0, 255), (128, 127), (1, 0), (255, 255)] {
            left[offset] = a;
            let mut right = left;
            right[offset] = b;
            p.memory.write(u64::from(LEFT), &left).unwrap();
            p.memory.write(u64::from(RIGHT), &right).unwrap();
            for count in [offset, offset + 1, 600] {
                let before = prepare(&mut p, LEFT, RIGHT, u32::try_from(count).unwrap());
                let expected = left[..count].cmp(&right[..count]);
                let difference = if count > offset {
                    i32::from(a) - i32::from(b)
                } else {
                    0
                };
                assert_eq!(difference.cmp(&0), expected);
                success(&mut p, before, difference);
            }
        }
    }
    p.memory.write(u64::from(LEFT), b"a\0a\0a\0").unwrap();
    let before = prepare(&mut p, LEFT, LEFT + 2, 4);
    success(&mut p, before, 0);
}

#[test]
fn zero_count_and_exact_bounds_do_not_need_extra_access_or_writes() {
    let mut p = load();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7000_2020, &88_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    for (left, right) in [(0, u32::MAX), (0x7000_0000, 0x7100_0000)] {
        let before = prepare(&mut p, left, right, 0);
        success(&mut p, before, 0);
    }
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, u32::MAX, 0x0040_2fff, 1);
    success(&mut p, before, 0);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    p.memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    assert_eq!(p.last_error().unwrap(), 77);
    let mut errno = [0; 4];
    p.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 88);
}

#[test]
fn entire_requested_spans_and_call_frame_are_checked_before_result() {
    let mut p = load();
    p.memory.write(u64::from(LEFT), &[1, 2]).unwrap();
    for (left, right, count) in [(LEFT, 0x0040_2fff, 2), (0, RIGHT, 1), (LEFT, u32::MAX, 2)] {
        let before = prepare(&mut p, left, right, count);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    prepare(&mut p, LEFT, RIGHT, 0);
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let before = p.cpu;
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
}

#[test]
fn bounded_comparison_accepts_its_limit_and_rejects_larger_counts() {
    let mut p = load();
    p.memory
        .map_zeroed(0x2000_0000, PAGE_SIZE * 16, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, 0x2000_0000, 0x2000_0000, 65536);
    success(&mut p, before, 0);
    for count in [65537, u32::MAX] {
        let before = prepare(&mut p, 0, 0, count);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
}
