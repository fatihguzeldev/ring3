use super::windows_format_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_025c;
const STACK: u32 = 0x1000_ff00;
const FORMAT: u32 = 0x0040_2180;
const OUTPUT: u32 = 0x0040_2400;

fn load() -> Process32 {
    Process32::load(&windows_format_executable::pe32(), 64).unwrap()
}
fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (i, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}
fn prepare(p: &mut Process32, output: u32, format: u32, values: &[u32]) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    words(p, STACK, &[0x0040_1000, output, format]);
    words(p, STACK + 12, values);
    p.cpu
}
fn bytes(p: &Process32, address: u32, count: usize) -> Vec<u8> {
    let mut bytes = vec![0; count];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}
fn success(p: &mut Process32, mut before: Cpu32, length: u32) {
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    before.eip = 0x0040_1000;
    before.set_register(Register32::Esp, STACK + 4);
    before.set_register(Register32::Eax, length);
    assert_eq!(p.cpu, before);
}

#[test]
fn imported_formatter_uses_cdecl_whole_or_stepwise() {
    for budget in [1, 100] {
        let mut p = load();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 100);
        }
        assert_eq!(counts, (7, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 10);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        assert_eq!(bytes(&p, OUTPUT, 11), b"-42:ABCD:%\0".to_vec());
    }
}

#[test]
fn bare_formats_preserve_errors_cpu_and_input_bytes() {
    let mut p = load();
    p.memory
        .write(u64::from(FORMAT), b"%d/%i/%u/%x/%X/%s/%%\0")
        .unwrap();
    p.memory.write(0x0040_2300, b"\x80\xff\0").unwrap();
    words(&mut p, 0x7ffd_e034, &[77]);
    words(&mut p, 0x7000_2020, &[88]);
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    let before = prepare(
        &mut p,
        OUTPUT,
        FORMAT,
        &[
            0x8000_0000,
            0x7fff_ffff,
            u32::MAX,
            u32::MAX,
            0xabcd,
            0x0040_2300,
        ],
    );
    let pages = p.memory.mapped_pages();
    let expected = b"-2147483648/2147483647/4294967295/ffffffff/ABCD/\x80\xff/%\0";
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, before);
    success(&mut p, before, u32::try_from(expected.len() - 1).unwrap());
    assert_eq!(bytes(&p, OUTPUT, expected.len()), expected);
    assert_eq!(p.memory.mapped_pages(), pages);
    for page in [0x7ffd_e000, 0x7000_2000] {
        p.memory
            .protect(page, 4096, Permissions::READ_WRITE)
            .unwrap();
    }
    assert_eq!(p.last_error().unwrap(), 77);
    assert_eq!(bytes(&p, 0x7000_2020, 4), 88_u32.to_le_bytes());
}

fn refuses(p: &mut Process32, before: Cpu32, fault: bool) {
    let old = bytes(p, OUTPUT, 1024);
    let run = p.run(1);
    if fault {
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    } else {
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    }
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(bytes(p, OUTPUT, 1024), old);
}

#[test]
fn payload_and_input_bounds_reject_before_writes() {
    let mut p = load();
    p.memory
        .map_zeroed(0x3000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(FORMAT), b"%s\0").unwrap();
    for length in [1023, 1024] {
        let mut text = vec![b'a'; length];
        text.push(0);
        p.memory.write(0x3000_0000, &text).unwrap();
        let before = prepare(&mut p, OUTPUT, FORMAT, &[0x3000_0000]);
        if length == 1023 {
            success(&mut p, before, 1023);
            assert_eq!(bytes(&p, OUTPUT, 1024), text);
        } else {
            refuses(&mut p, before, false);
        }
    }
    p.memory.write(0x3000_0000, &vec![b'a'; 1023]).unwrap();
    p.memory.write(0x3000_03ff, &[0]).unwrap();
    p.memory.write(u64::from(FORMAT), b"%sX\0").unwrap();
    let before = prepare(&mut p, OUTPUT, FORMAT, &[0x3000_0000]);
    refuses(&mut p, before, false);
    p.memory.write(0x3000_0000, &vec![b'%'; 4096]).unwrap();
    for (format, length) in [(0x3000_0000, 4096), (FORMAT, 0)] {
        if length == 0 {
            p.memory.write(u64::from(FORMAT), b"\0").unwrap();
        }
        let before = prepare(&mut p, OUTPUT, format, &[]);
        if length == 0 {
            success(&mut p, before, 0);
            assert_eq!(bytes(&p, OUTPUT, 1), [0]);
        } else {
            refuses(&mut p, before, false);
        }
    }
    for format in [
        b"%c\0".as_slice(),
        b"%n\0",
        b"%p\0",
        b"%#08x\0",
        b"%.2s\0",
        b"%ld\0",
        b"%\0",
    ] {
        p.memory.write(u64::from(FORMAT), format).unwrap();
        let before = prepare(&mut p, OUTPUT, FORMAT, &[1]);
        refuses(&mut p, before, false);
    }
}

#[test]
fn input_and_output_faults_are_atomic_and_write_only_outputs_work() {
    let mut p = load();
    p.memory.write(u64::from(FORMAT), b"%s\0").unwrap();
    for (output, format, args, fault) in [
        (OUTPUT, FORMAT, vec![0xdead_beef], true),
        (OUTPUT, FORMAT, vec![0], false),
        (0, FORMAT, vec![FORMAT], false),
        (OUTPUT, 0, vec![], false),
        (OUTPUT, u32::MAX, vec![], true),
        (0xdead_beef, FORMAT, vec![FORMAT], true),
    ] {
        let before = prepare(&mut p, output, format, &args);
        refuses(&mut p, before, fault);
    }
    p.memory.write(u64::from(FORMAT), b"abcdef\0").unwrap();
    p.memory
        .map_zeroed(0x3000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x3000_0ffd, b"xyz").unwrap();
    let before = prepare(&mut p, 0x3000_0ffd, FORMAT, &[]);
    refuses(&mut p, before, true);
    assert_eq!(bytes(&p, 0x3000_0ffd, 3), b"xyz");
    p.memory
        .protect(
            0x3000_0000,
            4096,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    let before = prepare(&mut p, 0x3000_0001, FORMAT, &[]);
    success(&mut p, before, 6);
    p.memory
        .protect(0x3000_0000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(bytes(&p, 0x3000_0001, 7), b"abcdef\0");
    let before = prepare(&mut p, 0x3000_0001, FORMAT, &[]);
    refuses(&mut p, before, true);
}

#[test]
fn output_snapshots_aliased_inputs_and_rereads_aliased_return() {
    let mut p = load();
    for output in [
        FORMAT,
        0x0040_2300,
        STACK + 12,
        STACK,
        0x7ffd_e034,
        0x7000_2020,
    ] {
        p.memory.write(u64::from(FORMAT), b"%s:%x\0").unwrap();
        p.memory.write(0x0040_2300, b"test\0").unwrap();
        let mut expected = prepare(&mut p, output, FORMAT, &[0x0040_2300, 0xab]);
        let run = p.run(1);
        assert_eq!(run.api_calls, 1);
        assert_eq!(
            run.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        expected.eip = if output == STACK {
            u32::from_le_bytes(*b"test")
        } else {
            0x0040_1000
        };
        expected.set_register(Register32::Esp, STACK + 4);
        expected.set_register(Register32::Eax, 7);
        assert_eq!(p.cpu, expected);
        assert_eq!(bytes(&p, output, 8), b"test:ab\0");
    }
}

#[test]
fn end_of_guest_frame_allows_unused_varargs_and_checks_consumed_words() {
    let mut p = load();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(FORMAT), b"ok\0").unwrap();
    prepare(&mut p, OUTPUT, FORMAT, &[]);
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    words(&mut p, 0xffff_fff4, &[0x0040_1000, OUTPUT, FORMAT]);
    assert_eq!(p.run(1).api_calls, 1);
    assert_eq!(p.cpu.register(Register32::Esp), 0xffff_fff8);
    assert_eq!(bytes(&p, OUTPUT, 3), b"ok\0");
    p.memory.write(u64::from(FORMAT), b"%x\0").unwrap();
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, 0xffff_fff4);
    let before = p.cpu;
    refuses(&mut p, before, true);
    p.cpu.set_register(Register32::Esp, 0xffff_fff8);
    let before = p.cpu;
    refuses(&mut p, before, true);
    p.memory.write(u64::from(FORMAT), b"ok\0").unwrap();
    let before = prepare(&mut p, 0xffff_fffe, FORMAT, &[]);
    refuses(&mut p, before, true);
    let before = prepare(&mut p, 0xffff_fffd, FORMAT, &[]);
    success(&mut p, before, 2);
    assert_eq!(bytes(&p, 0xffff_fffd, 3), b"ok\0");
}
