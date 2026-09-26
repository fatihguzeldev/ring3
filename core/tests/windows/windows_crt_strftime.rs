use super::imported_executable;

use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const IAT: u32 = 0x0040_2060;
const STRFTIME: u32 = 0x7000_01fc;
const STACK: u32 = 0x1000_ff00;
const FORMAT: u32 = 0x0040_22c0;
const TIME: u32 = 0x0040_2300;
const OUTPUT: u32 = 0x0040_2380;
const RETURN: u32 = 0x0040_1000;

fn read(process: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn write(process: &mut Process32, address: u32, bytes: &[u8]) {
    process.memory.write(u64::from(address), bytes).unwrap();
}

fn word(process: &Process32, address: u32) -> u32 {
    u32::from_le_bytes(read(process, address, 4).try_into().unwrap())
}

fn load() -> Process32 {
    let bytes = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["strftime"]);
    let process = Process32::load(&bytes, 32).unwrap();
    assert_eq!(word(&process, IAT), STRFTIME);
    process
}

fn prepare(process: &mut Process32, output: u32, maxsize: u32, format: u32, time: u32) {
    process.cpu.eip = word(process, IAT);
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 0x5555_5555);
    for (index, value) in [RETURN, output, maxsize, format, time]
        .into_iter()
        .enumerate()
    {
        write(
            process,
            STACK + u32::try_from(index).unwrap() * 4,
            &value.to_le_bytes(),
        );
    }
}

fn set_time(process: &mut Process32, fields: [u32; 9]) {
    for (index, field) in fields.into_iter().enumerate() {
        write(
            process,
            TIME + u32::try_from(index).unwrap() * 4,
            &field.to_le_bytes(),
        );
    }
}

fn completed(process: &mut Process32, expected: u32) {
    let mut cpu = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    cpu.eip = RETURN;
    cpu.set_register(Register32::Esp, STACK + 4);
    cpu.set_register(Register32::Eax, expected);
    assert_eq!(process.cpu, cpu);
}

#[test]
fn crt_strftime_formats_observed_tutorial_clock_and_returns_length_without_nul() {
    let mut process = load();
    write(&mut process, FORMAT, b"%a %b %d %H:%M\0");
    for (fields, expected) in [
        (
            [38, 4, 0, 1, 0, 70, 4, 0, 0],
            b"Thu Jan 01 00:04\0".as_slice(),
        ),
        (
            [56, 34, 12, 29, 1, 100, 2, 59, 0],
            b"Tue Feb 29 12:34\0".as_slice(),
        ),
    ] {
        set_time(&mut process, fields);
        write(&mut process, OUTPUT, &[0x55; 24]);
        prepare(&mut process, OUTPUT, 256, FORMAT, TIME);
        completed(&mut process, 16);
        assert_eq!(read(&process, OUTPUT, 17), expected);
        assert_eq!(read(&process, OUTPUT + 17, 7), [0x55; 7]);
    }
}

#[test]
fn crt_strftime_small_output_and_invalid_format_leave_destination_unchanged() {
    let mut process = load();
    set_time(&mut process, [38, 4, 0, 1, 0, 70, 4, 0, 0]);
    write(&mut process, FORMAT, b"%a %b %d %H:%M\0");
    write(&mut process, OUTPUT, &[0x55; 24]);
    prepare(&mut process, OUTPUT, 16, FORMAT, TIME);
    completed(&mut process, 0);
    assert_eq!(read(&process, OUTPUT, 24), [0x55; 24]);

    prepare(&mut process, OUTPUT, 17, FORMAT, TIME);
    completed(&mut process, 16);
    assert_eq!(read(&process, OUTPUT, 17), b"Thu Jan 01 00:04\0");
    write(&mut process, OUTPUT, &[0x55; 24]);

    write(&mut process, FORMAT, b"%Q\0");
    prepare(&mut process, OUTPUT, 256, FORMAT, TIME);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(result.reason, ProcessStop::UnsupportedApi { .. }));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(read(&process, OUTPUT, 24), [0x55; 24]);

    write(&mut process, FORMAT, b"%a\0");
    set_time(&mut process, [38, 4, 0, 1, 12, 70, 4, 0, 0]);
    prepare(&mut process, OUTPUT, 256, FORMAT, TIME);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(result.reason, ProcessStop::UnsupportedApi { .. }));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(read(&process, OUTPUT, 24), [0x55; 24]);
}

#[test]
fn crt_strftime_bad_tm_pointer_faults_without_output_or_cpu_change() {
    let mut process = load();
    write(&mut process, FORMAT, b"%a %b %d %H:%M\0");
    write(&mut process, OUTPUT, &[0x55; 24]);
    prepare(&mut process, OUTPUT, 256, FORMAT, u32::MAX - 1);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(read(&process, OUTPUT, 24), [0x55; 24]);
}
