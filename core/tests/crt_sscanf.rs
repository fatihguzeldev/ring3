#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01b8;
const STACK: u32 = 0x1000_ef00;
const INPUT: u32 = 0x0040_2300;
const FORMAT: u32 = 0x0040_2180;
const OUTPUT: u32 = 0x0040_2400;
const ERRNO: u32 = 0x7000_2020;

fn process(input: &[u8], format: &[u8]) -> Process32 {
    let mut process = Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["sscanf"]),
        32,
    )
    .unwrap();
    process.memory.write(u64::from(INPUT), input).unwrap();
    process.memory.write(u64::from(FORMAT), format).unwrap();
    process
        .memory
        .write(u64::from(OUTPUT), &0x1234_5678_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(u64::from(ERRNO), &77_u32.to_le_bytes())
        .unwrap();
    process
}

fn prepare(process: &mut Process32, input: u32, format: u32, output: u32) {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    let frame: Vec<_> = [0x0040_1000_u32, input, format, output]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    process.memory.write(u64::from(STACK), &frame).unwrap();
}

fn word(process: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn exact_decimal_format_assigns_one_i32_and_keeps_cdecl_arguments() {
    for (input, expected) in [
        (&b"123tail\0"[..], 123_i32),
        (&b" \t-42x\0"[..], -42),
        (&b"+2147483647\0"[..], i32::MAX),
        (&b"-2147483648\0"[..], i32::MIN),
    ] {
        let mut process = process(input, b"%d\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu.register(Register32::Eax), 1);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 4);
        assert_eq!(word(&process, OUTPUT), expected.cast_unsigned());
        assert_eq!(word(&process, ERRNO), 77);
    }
}

#[test]
fn no_conversion_and_null_arguments_leave_destination_unchanged() {
    for (input, expected) in [(&b"abc\0"[..], 0), (&b" \t\0"[..], u32::MAX)] {
        let mut process = process(input, b"%d\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.register(Register32::Eax), expected);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
        assert_eq!(word(&process, ERRNO), 77);
    }
    for (input, format) in [(0, FORMAT), (INPUT, 0)] {
        let mut process = process(b"123\0", b"%d\0");
        prepare(&mut process, input, format, OUTPUT);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.register(Register32::Eax), u32::MAX);
        assert_eq!(word(&process, ERRNO), 22);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
}

#[test]
fn unsupported_format_overflow_and_invalid_output_stop_without_write() {
    for (input, format, output, unsupported) in [
        (&b"123\0"[..], &b"%o\0"[..], OUTPUT, true),
        (&b"2147483648\0"[..], &b"%d\0"[..], OUTPUT, true),
        (&b"123\0"[..], &b"%d\0"[..], 0, false),
    ] {
        let mut process = process(input, format);
        prepare(&mut process, INPUT, FORMAT, output);
        let before = process.cpu;
        let reason = process.run(1).reason;
        if unsupported {
            assert_eq!(reason, ProcessStop::UnsupportedApi { address: API });
        } else {
            assert!(matches!(
                reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
}

#[test]
fn float_format_assigns_finite_f32_and_keeps_cdecl_arguments() {
    for (input, expected) in [
        (&b"1.25tail\0"[..], 1.25_f32),
        (&b" \t-.5!\0"[..], -0.5),
        (&b"+2e2tail\0"[..], 200.0),
        (&b"1.\0"[..], 1.0),
    ] {
        let mut process = process(input, b"%f\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu.register(Register32::Eax), 1);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 4);
        assert_eq!(word(&process, OUTPUT), expected.to_bits());
    }
}

#[test]
fn float_nonmatch_and_fault_preserve_destination() {
    for (input, expected) in [(&b"text\0"[..], 0), (&b" \t\0"[..], u32::MAX)] {
        let mut process = process(input, b"%f\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.register(Register32::Eax), expected);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
    for (input, output, unsupported) in [
        (&b"1e99\0"[..], OUTPUT, true),
        (&b"1.5\0"[..], 0, false),
        (&b"1e+\0"[..], OUTPUT, true),
    ] {
        let mut process = process(input, b"%f\0");
        prepare(&mut process, INPUT, FORMAT, output);
        let before = process.cpu;
        let reason = process.run(1).reason;
        if unsupported {
            assert_eq!(reason, ProcessStop::UnsupportedApi { address: API });
        } else {
            assert!(matches!(
                reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
}

#[test]
fn unsigned_format_assigns_u32_and_keeps_cdecl_arguments() {
    for (input, expected) in [
        (&b"7tail\0"[..], 7_u32),
        (&b" \t+12x\0"[..], 12),
        (&b"-1\0"[..], u32::MAX),
        (&b"4294967295\0"[..], u32::MAX),
        (&b"0123\0"[..], 123),
    ] {
        let mut process = process(input, b"%u\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu.register(Register32::Eax), 1);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 4);
        assert_eq!(word(&process, OUTPUT), expected);
    }
}

#[test]
fn unsigned_nonmatch_overflow_and_fault_preserve_destination() {
    for (input, expected) in [(&b"text\0"[..], 0), (&b" \t\0"[..], u32::MAX)] {
        let mut process = process(input, b"%u\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.register(Register32::Eax), expected);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
    for (input, output, unsupported) in [
        (&b"4294967296\0"[..], OUTPUT, true),
        (&b"12\0"[..], 0, false),
    ] {
        let mut process = process(input, b"%u\0");
        prepare(&mut process, INPUT, FORMAT, output);
        let before = process.cpu;
        let reason = process.run(1).reason;
        if unsupported {
            assert_eq!(reason, ProcessStop::UnsupportedApi { address: API });
        } else {
            assert!(matches!(
                reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
}

#[test]
fn hex_format_assigns_32_bit_pattern_and_keeps_cdecl_arguments() {
    for (input, expected) in [
        (&b"ABCDEF12tail\0"[..], 0xabcd_ef12_u32),
        (&b" \t0x7f!\0"[..], 0x7f),
        (&b"0Xffffffff\0"[..], u32::MAX),
        (&b"-a\0"[..], u32::MAX - 9),
        (&b"+00aBc\0"[..], 0xabc),
    ] {
        let mut process = process(input, b"%x\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        let result = process.run(1);
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!((result.instructions, result.api_calls), (0, 1));
        assert_eq!(process.cpu.register(Register32::Eax), 1);
        assert_eq!(process.cpu.register(Register32::Esp), STACK + 4);
        assert_eq!(word(&process, OUTPUT), expected);
    }
}

#[test]
fn hex_nonmatch_overflow_and_fault_preserve_destination() {
    for (input, expected) in [(&b"xyz\0"[..], 0), (&b" \t\0"[..], u32::MAX)] {
        let mut process = process(input, b"%x\0");
        prepare(&mut process, INPUT, FORMAT, OUTPUT);
        assert_eq!(
            process.run(1).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        assert_eq!(process.cpu.register(Register32::Eax), expected);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
    for (input, output, unsupported) in [
        (&b"100000000\0"[..], OUTPUT, true),
        (&b"0x\0"[..], OUTPUT, true),
        (&b"ab\0"[..], 0, false),
    ] {
        let mut process = process(input, b"%x\0");
        prepare(&mut process, INPUT, FORMAT, output);
        let before = process.cpu;
        let reason = process.run(1).reason;
        if unsupported {
            assert_eq!(reason, ProcessStop::UnsupportedApi { address: API });
        } else {
            assert!(matches!(
                reason,
                ProcessStop::Stopped(StopReason::MemoryFault(_))
            ));
        }
        assert_eq!(process.cpu, before);
        assert_eq!(word(&process, OUTPUT), 0x1234_5678);
    }
}
