#[path = "support/case_compare_executable.rs"]
mod case_compare_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0174;
const STACK: u32 = 0x1000_ff00;
const LEFT: u32 = 0x0040_2180;
const RIGHT: u32 = 0x0040_2190;

fn process() -> Process32 {
    Process32::load(&case_compare_executable::pe32(), 64).unwrap()
}

fn prepare(p: &mut Process32, left: u32, right: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, left, right].into_iter().enumerate() {
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
fn imported_case_comparison_runs_whole_or_stepwise() {
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
        assert_eq!(counts, (10, 2));
        assert_eq!(p.cpu.register(Register32::Esi), 0);
        assert_eq!(p.cpu.register(Register32::Eax), 2);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

#[test]
fn c_locale_folds_ascii_letters_but_preserves_punctuation_high_bytes_and_state() {
    let mut p = process();
    p.memory.write(0x7000_2020, &88_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    for (left, right, expected) in [
        (b"MiXeD\0".as_slice(), b"mixed\0".as_slice(), 0),
        (b"\0unused", b"\0different", 0),
        (b"a\0", b"AB\0", -98),
        (b"JOHNSTON\0", b"JOHN_HENRY\0", 20),
        (b"Z\0", b"[\0", 31),
        (b"A\0", b"_\0", 2),
        (b"\xc4\0", b"\xe4\0", -32),
        (b"\xff\0", b"\x7f\0", 128),
    ] {
        for (left, right, expected) in [(left, right, expected), (right, left, -expected)] {
            p.memory.write(u64::from(LEFT), left).unwrap();
            p.memory.write(u64::from(RIGHT), right).unwrap();
            let before = prepare(&mut p, LEFT, RIGHT);
            assert_eq!(p.run(0).api_calls, 0);
            assert_eq!(p.cpu, before);
            success(&mut p, before, expected);
            let mut bytes = vec![0; left.len()];
            p.memory.read(u64::from(LEFT), &mut bytes).unwrap();
            assert_eq!(bytes, left);
        }
    }
    for (upper, lower) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        .iter()
        .zip(b"abcdefghijklmnopqrstuvwxyz")
    {
        p.memory.write(u64::from(LEFT), &[*upper, 0]).unwrap();
        p.memory.write(u64::from(RIGHT), &[*lower, 0]).unwrap();
        let before = prepare(&mut p, LEFT, RIGHT);
        success(&mut p, before, 0);
    }
    p.memory.write(u64::from(LEFT), b"Aa\0").unwrap();
    let before = prepare(&mut p, LEFT, LEFT + 1);
    success(&mut p, before, 97);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    let mut bytes = [0; 4];
    p.memory.read(0x7000_2020, &mut bytes).unwrap();
    assert_eq!(bytes, 88_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn compared_byte_faults_are_atomic_but_early_results_do_not_read_past_edges() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    for (a, b, expected) in [(0, 0, 0), (b'Z', b'[', 31), (0x80, 0x7f, 1)] {
        p.memory.write(u64::from(u32::MAX), &[a]).unwrap();
        p.memory.write(0x0040_2fff, &[b]).unwrap();
        let before = prepare(&mut p, u32::MAX, 0x0040_2fff);
        success(&mut p, before, expected);
    }
    p.memory.write(u64::from(u32::MAX), b"A").unwrap();
    p.memory.write(0x0040_2fff, b"A").unwrap();
    p.memory.write(u64::from(RIGHT), b"aa\0").unwrap();
    for (left, right) in [
        (0x6000_0000, RIGHT),
        (LEFT, 0x6000_0000),
        (0x0040_2fff, RIGHT),
        (u32::MAX, RIGHT),
    ] {
        let before = prepare(&mut p, left, right);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
    }
    for (left, right) in [(0, RIGHT), (LEFT, 0)] {
        let before = prepare(&mut p, left, right);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: API }
        );
        assert_eq!(p.cpu, before);
    }
    p.memory.write(u64::from(LEFT), b"AA\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, LEFT, RIGHT);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    success(&mut p, before, 0);
    prepare(&mut p, LEFT, RIGHT);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn last_bounded_comparison_can_finish_but_equal_unterminated_prefix_is_unsupported() {
    let mut p = process();
    p.memory
        .map_zeroed(0x2000_0000, 131_072, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'A'; 65536]).unwrap();
    p.memory.write(0x2001_0000, &vec![b'a'; 65536]).unwrap();
    let before = prepare(&mut p, 0x2000_0000, 0x2001_0000);
    let run = p.run(1);
    assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory.write(0x2001_ffff, b"b").unwrap();
    success(&mut p, before, -1);
    p.memory.write(0x2000_ffff, &[0]).unwrap();
    p.memory.write(0x2001_ffff, &[0]).unwrap();
    let before = prepare(&mut p, 0x2000_0000, 0x2001_0000);
    success(&mut p, before, 0);
}
