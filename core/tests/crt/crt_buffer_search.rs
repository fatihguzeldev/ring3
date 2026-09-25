use super::imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_01c8;
const STACK: u32 = 0x1000_ff00;
const BUFFER: u32 = 0x0040_2180;

fn process() -> Process32 {
    let code = [
        0x6a, 6, 0x6a, b'.', 0x68, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83,
        0xc4, 12, 0xcc,
    ];
    Process32::load(
        &imported_executable::pe32(&code, "MSVCRT.dll", &["memchr"]),
        48,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, address: u32, value: u32, count: u32) -> Cpu32 {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, word) in [0x0040_1000, address, value, count].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    process.cpu
}

fn success(process: &mut Process32, mut expected: Cpu32, value: u32) {
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(process.cpu, expected);
}

#[test]
fn imported_memchr_finds_the_first_byte_in_a_binary_buffer() {
    let mut process = process();
    process.memory.write(u64::from(BUFFER), b"a.b.cd").unwrap();
    let result = process.run(30);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!(result.api_calls, 1);
    assert_eq!(process.cpu.register(Register32::Eax), BUFFER + 1);
}

#[test]
fn search_obeys_count_zero_bytes_and_unsigned_character_conversion() {
    let mut process = process();
    process
        .memory
        .write(u64::from(BUFFER), b"a\0.\xff.")
        .unwrap();
    for (character, count, expected) in [
        (u32::from(b'.'), 2, 0),
        (u32::from(b'.'), 3, BUFFER + 2),
        (0, 5, BUFFER + 1),
        (0x1ff, 5, BUFFER + 3),
        (u32::from(b'.'), 5, BUFFER + 2),
        (u32::from(b'z'), 5, 0),
    ] {
        let before = prepare(&mut process, BUFFER, character, count);
        success(&mut process, before, expected);
    }
    for address in [0, 0x7000_0000, u32::MAX] {
        let before = prepare(&mut process, address, u32::from(b'.'), 0);
        success(&mut process, before, 0);
    }
    let mut bytes = [0; 5];
    process.memory.read(u64::from(BUFFER), &mut bytes).unwrap();
    assert_eq!(&bytes, b"a\0.\xff.");
}

#[test]
fn exact_last_byte_is_read_but_invalid_spans_are_rejected() {
    let mut process = process();
    process.memory.write(0x0040_2fff, b".").unwrap();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut process, 0x0040_2fff, u32::from(b'.'), 1);
    success(&mut process, before, 0x0040_2fff);
    for (address, count) in [(0x0040_2fff, 2), (0, 1), (u32::MAX, 2)] {
        let before = prepare(&mut process, address, u32::from(b'.'), count);
        let result = process.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
    let before = prepare(&mut process, BUFFER, u32::from(b'.'), 65537);
    let result = process.run(1);
    assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
    assert_eq!(process.cpu, before);
}
