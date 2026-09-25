use super::character_search_executable;
use super::imported_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

#[test]
fn imported_first_character_and_terminator_agree_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = Process32::load(&character_search_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = p.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 50);
        }
        assert_eq!(counts, (10, 2));
        assert_eq!(p.cpu.register(Register32::Ebx), 0x0040_2180);
        assert_eq!(p.cpu.register(Register32::Eax), 0x0040_2184);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

const API: u32 = 0x7000_0158;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["strchr"]),
        64,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, source: u32, character: u32) {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000_u32, source, character].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn find(process: &mut Process32, source: u32, character: u32, expected_pointer: u32) {
    prepare(process, source, character);
    let mut expected = process.cpu;
    let pages = process.memory.mapped_pages();
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, expected_pointer);
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    assert_eq!(process.cpu, expected);
    assert_eq!(process.memory.mapped_pages(), pages);
}

fn fault(process: &mut Process32) {
    let before = process.cpu;
    let pages = process.memory.mapped_pages();
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    assert_eq!(process.memory.mapped_pages(), pages);
}

#[test]
fn first_positions_match_byte_array_oracle_with_low_byte_integer_conversion() {
    let mut p = process();
    let mut bytes: Vec<u8> = (1..=255).rev().collect();
    bytes.extend_from_slice(&[0x80, b'a', 0, b'z']);
    p.memory.write(u64::from(SOURCE), &bytes).unwrap();
    p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    for target in 0..=255_u32 {
        let index = bytes.iter().position(|&b| u32::from(b) == target).unwrap();
        for high in [0, 0x1234_5600, 0xffff_ff00] {
            find(
                &mut p,
                SOURCE,
                high | target,
                SOURCE + u32::try_from(index).unwrap(),
            );
        }
    }
    for (offset, target, expected) in [
        (256, b'z', 0),
        (257, b'z', 0),
        (257, 0, SOURCE + 257),
        (258, b'z', SOURCE + 258),
    ] {
        find(&mut p, SOURCE + offset, u32::from(target), expected);
    }
    let mut after = vec![0; bytes.len()];
    p.memory.read(u64::from(SOURCE), &mut after).unwrap();
    assert_eq!(after, bytes);
    assert_eq!(p.last_error().unwrap(), 77);
    let mut error = [0; 4];
    p.memory.read(0x7000_2020, &mut error).unwrap();
    assert_eq!(error, 123_u32.to_le_bytes());
    find(&mut p, STACK, 0, STACK);
    find(&mut p, STACK + 4, 0xff, STACK + 5);
    find(&mut p, 0x7000_2020, 123, 0x7000_2020);
    find(&mut p, 0x7ffd_e034, 77, 0x7ffd_e034);
}

#[test]
fn match_and_nul_stop_reads_at_page_and_guest32_edges() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for address in [0x0040_2fff, u32::MAX] {
        p.memory.write(u64::from(address), &[0x80]).unwrap();
        find(&mut p, address, 0x80, address);
        prepare(&mut p, address, 0x81);
        fault(&mut p);
        p.memory.write(u64::from(address), &[0]).unwrap();
        find(&mut p, address, 0, address);
        find(&mut p, address, b'x'.into(), 0);
    }
    p.memory.write(0x0040_2fff, b"x").unwrap();
    p.memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x0040_3000, b"y\0").unwrap();
    find(&mut p, 0x0040_2fff, b'y'.into(), 0x0040_3000);
    find(&mut p, 0x0040_2fff, 0, 0x0040_3001);
    p.memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    find(&mut p, 0x0040_2fff, b'x'.into(), 0x0040_2fff);
    prepare(&mut p, 0x0040_2fff, b'y'.into());
    fault(&mut p);
}

#[test]
fn invalid_source_frame_and_zero_budget_preserve_state() {
    let mut p = process();
    for source in [0, 0x6000_0000, 0x7000_0000] {
        prepare(&mut p, source, b'x'.into());
        let before = p.cpu;
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        fault(&mut p);
    }
    p.memory
        .protect(
            0x0040_2000,
            PAGE_SIZE,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        )
        .unwrap();
    prepare(&mut p, SOURCE, 0);
    fault(&mut p);
    prepare(&mut p, SOURCE, 0);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    fault(&mut p);
}

#[test]
fn scan_budget_includes_match_or_nul_and_never_reports_false_absence() {
    for source in [0x2000_0000, 0xffff_0000] {
        let mut p = process();
        p.memory
            .map_zeroed(u64::from(source), PAGE_SIZE * 16, Permissions::READ_WRITE)
            .unwrap();
        p.memory
            .write(u64::from(source), &vec![b'x'; 65536])
            .unwrap();
        prepare(&mut p, source, b'y'.into());
        let before = p.cpu;
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        p.memory.write(u64::from(source) + 65535, b"y").unwrap();
        find(&mut p, source, b'y'.into(), source + 65535);
        p.memory.write(u64::from(source) + 65535, &[0]).unwrap();
        find(&mut p, source, b'y'.into(), 0);
        find(&mut p, source, 0, source + 65535);
    }
}
