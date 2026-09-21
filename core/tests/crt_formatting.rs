#[path = "support/formatting_executable.rs"]
mod formatting_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0178;
const STACK: u32 = 0x1000_ff00;
const FORMAT: u32 = 0x0040_2180;
const VALUES: u32 = 0x0040_21c0;
const OUTPUT: u32 = 0x0040_2400;

fn process() -> Process32 {
    Process32::load(&formatting_executable::pe32(), 256).unwrap()
}

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn prepare(p: &mut Process32, args: [u32; 4]) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    words(p, STACK, &[0x0040_1000]);
    words(p, STACK + 4, &args);
    p.cpu
}

fn success(p: &mut Process32, mut expected: Cpu32, value: i32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value.cast_unsigned());
    assert_eq!(p.cpu, expected);
}

fn bytes(p: &Process32, address: u32, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; count];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn refuses(p: &mut Process32, before: Cpu32, memory_fault: bool) {
    let old = bytes(p, OUTPUT, 128);
    let run = p.run(1);
    if memory_fault {
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    } else {
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    }
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(p, OUTPUT, 128), old);
}

#[test]
fn imported_formatter_runs_whole_or_stepwise() {
    for budget in [1, 40] {
        let mut p = Process32::load(&formatting_executable::pe32(), 64).unwrap();
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
        assert_eq!(counts, (7, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 13);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 14];
        p.memory.read(0x0040_2200, &mut bytes).unwrap();
        assert_eq!(&bytes, b"ok:-42:ABCD:%\0");
    }
}

#[test]
fn bare_formats_use_guest_words_and_preserve_state() {
    let mut p = process();
    words(&mut p, 0x7000_2020, &[88]);
    words(&mut p, 0x7ffd_e034, &[77]);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    p.memory
        .write(u64::from(FORMAT), b"%d/%i/%u/%x/%X/%c/%s/%%\0")
        .unwrap();
    p.memory.write(0x0040_2300, b"\x80\xff\0").unwrap();
    words(
        &mut p,
        VALUES + 1,
        &[
            0x8000_0000,
            0x7fff_ffff,
            u32::MAX,
            u32::MAX,
            0xabcd,
            0x1234_0100,
            0x0040_2300,
        ],
    );
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES + 1]);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    let expected = b"-2147483648/2147483647/4294967295/ffffffff/ABCD/\0/\x80\xff/%\0";
    success(&mut p, before, i32::try_from(expected.len() - 1).unwrap());
    assert_eq!(bytes(&p, OUTPUT, expected.len()), expected);
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::READ).unwrap();
    }
    assert_eq!(bytes(&p, 0x7000_2020, 4), 88_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
    p.memory
        .write(u64::from(FORMAT), b"%d:%u:%x:%X:%c\0")
        .unwrap();
    words(&mut p, VALUES, &[0, 0, 0, 0, 0x1234_0141]);
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    success(&mut p, before, 9);
    assert_eq!(bytes(&p, OUTPUT, 10), b"0:0:0:0:A\0");
}

#[test]
fn legacy_capacity_rules_preserve_tail_bytes_and_do_not_require_unused_capacity() {
    let mut p = process();
    p.memory.write(u64::from(FORMAT), b"justfits\0").unwrap();
    for (count, result, expected) in [
        (0, -1, b"!".as_slice()),
        (1, -1, b"j!"),
        (7, -1, b"justfit!"),
        (8, 8, b"justfits!"),
        (9, 8, b"justfits\0!"),
        (u32::MAX, 8, b"justfits\0!"),
    ] {
        p.memory.write(u64::from(OUTPUT), &[b'!'; 16]).unwrap();
        let before = prepare(&mut p, [OUTPUT, count, FORMAT, 0]);
        success(&mut p, before, result);
        assert_eq!(bytes(&p, OUTPUT, expected.len()), expected);
    }
    let before = prepare(&mut p, [0, 0, FORMAT, 0]);
    success(&mut p, before, 8);
    let before = prepare(&mut p, [0x6000_0000, 0, FORMAT, 0]);
    success(&mut p, before, -1);
    p.memory.write(u64::from(FORMAT), b"\0").unwrap();
    let before = prepare(&mut p, [0x6000_0000, 0, FORMAT, 0]);
    success(&mut p, before, 0);
    let before = prepare(&mut p, [0, 1, FORMAT, 0]);
    refuses(&mut p, before, false);
}

#[test]
fn unsupported_formats_and_null_strings_do_not_partially_write() {
    let mut p = process();
    p.memory.write(u64::from(OUTPUT), &[b'!'; 128]).unwrap();
    for format in [
        b"ok%\0".as_slice(),
        b"ok%08x\0",
        b"ok%.2s\0",
        b"ok%*s\0",
        b"ok%ld\0",
        b"ok%f\0",
        b"ok%n\0",
        b"ok%p\0",
        b"ok%o\0",
        b"ok%S\0",
        b"ok%1$s\0",
    ] {
        p.memory.write(u64::from(FORMAT), format).unwrap();
        let before = prepare(&mut p, [OUTPUT, 1, FORMAT, 0x6000_0000]);
        refuses(&mut p, before, false);
    }
    p.memory.write(u64::from(FORMAT), b"ok%s\0").unwrap();
    words(&mut p, VALUES, &[0]);
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    refuses(&mut p, before, false);
    let before = prepare(&mut p, [OUTPUT, 128, 0, VALUES]);
    refuses(&mut p, before, false);
}

#[test]
fn input_and_output_faults_are_atomic_and_retryable_even_when_truncated() {
    let mut p = process();
    p.memory.write(u64::from(OUTPUT), &[b'!'; 128]).unwrap();
    p.memory.write(u64::from(FORMAT), b"ok%s\0").unwrap();
    words(&mut p, VALUES, &[0x6000_0000]);
    for (format, values, capacity) in [
        (0x6000_0000, VALUES, 128),
        (FORMAT, 0x6000_0000, 128),
        (FORMAT, VALUES, 128),
        (FORMAT, VALUES, 1),
        (FORMAT, VALUES, 0),
    ] {
        let before = prepare(&mut p, [OUTPUT, capacity, format, values]);
        refuses(&mut p, before, true);
    }
    p.memory
        .map_zeroed(0x6000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_0000, b"value\0").unwrap();
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    success(&mut p, before, 7);
    p.memory.write(0x6000_0ffc, b"!!!!").unwrap();
    p.memory
        .protect(0x6000_1000, 4096, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, [0x6000_0ffc, 128, FORMAT, VALUES]);
    refuses(&mut p, before, true);
    assert_eq!(bytes(&p, 0x6000_0ffc, 4), b"!!!!");
    p.memory
        .protect(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    success(&mut p, before, 7);
    assert_eq!(bytes(&p, 0x6000_0ffc, 8), b"okvalue\0");
    prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    p.cpu.set_register(Register32::Esp, 0x1000_fff0);
    let before = p.cpu;
    refuses(&mut p, before, true);
}

#[test]
fn last_guest_byte_and_last_argument_word_do_not_require_following_memory() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(u32::MAX), b"\0").unwrap();
    let before = prepare(&mut p, [OUTPUT, 128, u32::MAX, 0]);
    success(&mut p, before, 0);
    p.memory.write(u64::from(FORMAT), b"%s\0").unwrap();
    words(&mut p, VALUES, &[u32::MAX]);
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    success(&mut p, before, 0);
    p.memory.write(u64::from(u32::MAX), b"a").unwrap();
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    refuses(&mut p, before, true);
    p.memory.write(u64::from(FORMAT), b"%u\0").unwrap();
    words(&mut p, 0xffff_fffc, &[7]);
    let before = prepare(&mut p, [u32::MAX, 1, FORMAT, 0xffff_fffc]);
    success(&mut p, before, 1);
    assert_eq!(bytes(&p, u32::MAX, 1), b"7");
    let before = prepare(&mut p, [u32::MAX, 2, FORMAT, 0xffff_fffc]);
    refuses(&mut p, before, true);
    assert_eq!(bytes(&p, u32::MAX, 1), b"7");
    p.memory.write(u64::from(FORMAT), b"%u%u\0").unwrap();
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, 0xffff_fffc]);
    refuses(&mut p, before, true);
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, 0xffff_fffd]);
    refuses(&mut p, before, true);
}

#[test]
fn input_snapshots_allow_aliases_and_return_frame_writes_remain_observable() {
    let mut p = process();
    p.memory.write(u64::from(FORMAT), b"%s%s\0").unwrap();
    p.memory.write(u64::from(OUTPUT), b"ab\0").unwrap();
    words(&mut p, VALUES, &[OUTPUT, OUTPUT + 1]);
    let before = prepare(&mut p, [OUTPUT, 128, FORMAT, VALUES]);
    success(&mut p, before, 3);
    assert_eq!(bytes(&p, OUTPUT, 4), b"abb\0");
    let before = prepare(&mut p, [FORMAT, 128, FORMAT, VALUES]);
    success(&mut p, before, 5);
    assert_eq!(bytes(&p, FORMAT, 6), b"abbbb\0");
    p.memory.write(u64::from(FORMAT), b"%u%u\0").unwrap();
    words(&mut p, VALUES, &[12, 34]);
    let before = prepare(&mut p, [VALUES, 128, FORMAT, VALUES]);
    success(&mut p, before, 4);
    assert_eq!(bytes(&p, VALUES, 5), b"1234\0");
    p.memory.write(u64::from(FORMAT), b"ABCD\0").unwrap();
    let mut expected = prepare(&mut p, [STACK, 4, FORMAT, 0]);
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x4443_4241;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, 4);
    assert_eq!(p.cpu, expected);
}

#[test]
fn bounded_format_source_and_rendered_lengths_are_independent() {
    let mut p = process();
    p.memory
        .map_zeroed(0x6000_0000, 139_264, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x6000_0000, &vec![b'a'; 4096]).unwrap();
    let before = prepare(&mut p, [OUTPUT, 0, 0x6000_0000, 0]);
    refuses(&mut p, before, false);
    p.memory.write(0x6000_0fff, b"\0").unwrap();
    let before = prepare(&mut p, [0, 0, 0x6000_0000, 0]);
    success(&mut p, before, 4095);
    p.memory.write(0x6000_1000, &vec![b'a'; 65536]).unwrap();
    p.memory.write(u64::from(FORMAT), b"%s!\0").unwrap();
    words(&mut p, VALUES, &[0x6000_1000]);
    let before = prepare(&mut p, [OUTPUT, 0, FORMAT, VALUES]);
    refuses(&mut p, before, false);
    p.memory.write(0x6001_0fff, b"\0").unwrap();
    let before = prepare(&mut p, [0x6001_1000, 65537, FORMAT, VALUES]);
    success(&mut p, before, 65536);
    assert_eq!(bytes(&p, 0x6001_1000, 65535), vec![b'a'; 65535]);
    assert_eq!(bytes(&p, 0x6002_0fff, 2), b"!\0");
    p.memory.write(u64::from(FORMAT), b"%s!!\0").unwrap();
    let before = prepare(&mut p, [OUTPUT, 0, FORMAT, VALUES]);
    refuses(&mut p, before, false);
}
