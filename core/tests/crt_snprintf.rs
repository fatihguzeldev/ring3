#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Cpu32, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_01bc;
const STACK: u32 = 0x1000_ff00;
const FORMAT: u32 = 0x0040_2180;
const TEXT: u32 = 0x0040_2300;
const OUTPUT: u32 = 0x0040_2400;

fn process() -> Process32 {
    let pe = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["_snprintf"]);
    Process32::load(&pe, 128).unwrap()
}

fn words(p: &mut Process32, address: u32, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        p.memory
            .write(u64::from(address) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn prepare(
    p: &mut Process32,
    destination: u32,
    capacity: u32,
    format: u32,
    values: &[u32],
) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    words(p, STACK, &[0x0040_1000, destination, capacity, format]);
    words(p, STACK + 16, values);
    p.cpu
}

fn output(p: &Process32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    bytes
}

fn success(p: &mut Process32, mut expected: Cpu32, value: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

#[test]
fn imported_snprintf_renders_guest_string_and_uses_cdecl_cleanup() {
    let mut p = process();
    p.memory.write(u64::from(FORMAT), b"%s\0").unwrap();
    p.memory.write(u64::from(TEXT), b"play\0").unwrap();
    p.memory.write(u64::from(OUTPUT), &[b'!'; 16]).unwrap();
    let before = prepare(&mut p, OUTPUT, 256, FORMAT, &[TEXT]);
    success(&mut p, before, 4);
    assert_eq!(output(&p, 6), b"play\0!");
}

#[test]
fn legacy_capacity_rules_do_not_terminate_exact_or_truncated_output() {
    let mut p = process();
    p.memory.write(u64::from(FORMAT), b"%s\0").unwrap();
    p.memory.write(u64::from(TEXT), b"play\0").unwrap();
    for (capacity, result, expected) in [
        (0, u32::MAX, b"!".as_slice()),
        (1, u32::MAX, b"p!"),
        (4, u32::MAX, b"play!"),
        (5, 4, b"play\0!"),
    ] {
        p.memory.write(u64::from(OUTPUT), &[b'!'; 16]).unwrap();
        let before = prepare(&mut p, OUTPUT, capacity, FORMAT, &[TEXT]);
        success(&mut p, before, result);
        assert_eq!(output(&p, expected.len()), expected);
    }
}

#[test]
fn unsupported_format_and_output_fault_leave_guest_state_untouched() {
    let mut p = process();
    p.memory.write(u64::from(OUTPUT), &[b'!'; 16]).unwrap();
    p.memory.write(u64::from(FORMAT), b"%08x\0").unwrap();
    let before = prepare(&mut p, OUTPUT, 8, FORMAT, &[42]);
    let result = p.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(output(&p, 16), vec![b'!'; 16]);

    p.memory.write(u64::from(FORMAT), b"%s\0").unwrap();
    p.memory.write(u64::from(TEXT), b"play\0").unwrap();
    let before = prepare(&mut p, 0x6000_0000, 5, FORMAT, &[TEXT]);
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(output(&p, 16), vec![b'!'; 16]);
}
