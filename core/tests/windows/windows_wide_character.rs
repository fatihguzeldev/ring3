use super::code_pages_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_0338;
const STACK: u32 = 0x1000_ef00;
const SOURCE: u32 = 0x1000_d000;
const OUTPUT: u32 = 0x1000_d100;
const USED: u32 = 0x1000_d200;

fn process() -> Process32 {
    Process32::load(&code_pages_executable::pe32(), 32).unwrap()
}

fn call(process: &mut Process32, args: [u32; 8]) -> u32 {
    let mut frame = Vec::from(0x0040_100e_u32.to_le_bytes());
    for arg in args {
        frame.extend_from_slice(&arg.to_le_bytes());
    }
    process.memory.write(u64::from(STACK), &frame).unwrap();
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    let run = process.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(process.cpu.register(Register32::Esp), STACK + 36);
    process.cpu.register(Register32::Eax)
}

fn utf16(process: &mut Process32, units: &[u16]) {
    let bytes: Vec<_> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    process.memory.write(u64::from(SOURCE), &bytes).unwrap();
}

#[test]
fn ansi_conversion_includes_terminator_and_reports_default_usage() {
    let mut process = process();
    utf16(&mut process, &[u16::from(b'A'), 0x20ac, 0x4e2d, 0]);
    let args = [0, 0, SOURCE, u32::MAX, OUTPUT, 8, 0, USED];
    assert_eq!(call(&mut process, args), 4);
    let mut bytes = [0; 4];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    assert_eq!(bytes, [b'A', 0x80, b'?', 0]);
    process.memory.read(u64::from(USED), &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 1);
}

#[test]
fn size_query_and_explicit_length_do_not_invent_terminator() {
    let mut process = process();
    utf16(&mut process, &[u16::from(b'A'), u16::from(b'B'), 0]);
    assert_eq!(
        call(&mut process, [1252, 0, SOURCE, u32::MAX, 0, 0, 0, 0]),
        3
    );
    assert_eq!(call(&mut process, [3, 0, SOURCE, 2, OUTPUT, 2, 0, 0]), 2);
    let mut bytes = [0xff; 3];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    assert_eq!(&bytes[..2], b"AB");
}

#[test]
fn custom_default_and_insufficient_buffer_preserve_output() {
    let mut process = process();
    utf16(&mut process, &[0x4e2d, 0]);
    process.memory.write(u64::from(OUTPUT), b"old").unwrap();
    process.memory.write(u64::from(USED), &[0; 4]).unwrap();
    process.memory.write(u64::from(USED + 4), b"!").unwrap();
    let args = [0, 0, SOURCE, u32::MAX, OUTPUT, 1, USED + 4, USED];
    assert_eq!(call(&mut process, args), 0);
    assert_eq!(process.last_error().unwrap(), 122);
    let mut bytes = [0; 3];
    process.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    assert_eq!(&bytes, b"old");
    assert_eq!(
        call(
            &mut process,
            [0, 0, SOURCE, u32::MAX, OUTPUT, 2, USED + 4, USED]
        ),
        2
    );
    process
        .memory
        .read(u64::from(OUTPUT), &mut bytes[..2])
        .unwrap();
    assert_eq!(&bytes[..2], b"!\0");
    process.memory.read(u64::from(USED), &mut bytes).unwrap();
    assert_eq!(bytes, [1, 0, 0]);
}

#[test]
fn invalid_parameters_report_error_without_conversion() {
    let mut process = process();
    utf16(&mut process, &[u16::from(b'A'), 0]);
    for args in [
        [0, 0, 0, u32::MAX, OUTPUT, 2, 0, 0],
        [0, 0, SOURCE, 0, OUTPUT, 2, 0, 0],
        [0, 0, SOURCE, u32::MAX, SOURCE, 2, 0, 0],
        [0, 0, SOURCE, u32::MAX, 0, 2, 0, 0],
    ] {
        assert_eq!(call(&mut process, args), 0);
        assert_eq!(process.last_error().unwrap(), 87);
    }
}
