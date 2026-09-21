#[path = "support/imported_executable.rs"]
mod imported_executable;

#[path = "support/buffer_copy_executable.rs"]
mod buffer_copy_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0150;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DESTINATION: u32 = 0x0040_2800;

#[test]
fn imported_buffer_copy_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = Process32::load(&buffer_copy_executable::pe32(), 32).unwrap();
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
        assert_eq!(counts, (13, 2));
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Ebx), 0x0040_2190);
        assert_eq!(p.cpu.register(Register32::Ecx), 0xff80_0061);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["memcpy"]),
        128,
    )
    .unwrap()
}

fn prepare(p: &mut Process32, destination: u32, source: u32, count: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000, destination, source, count]
        .into_iter()
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn read(p: &Process32, address: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn success(p: &mut Process32, mut expected: Cpu32, destination: u32, resume: u32) {
    let pages = p.memory.mapped_pages();
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = resume;
    expected.set_register(Register32::Eax, destination);
    expected.set_register(Register32::Esp, STACK + 4);
    assert_eq!(p.cpu, expected);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn exact_copy_matches_slices_across_chunks_and_pages_without_reading_destination() {
    let mut p = process();
    let source = 0x2000_0ffd;
    let destination = 0x3000_0001;
    for base in [0x2000_0000, 0x3000_0000] {
        p.memory
            .map_zeroed(base, PAGE_SIZE * 18, Permissions::READ_WRITE)
            .unwrap();
    }
    let bytes: Vec<_> = (0..65537)
        .map(|i| u8::try_from((i * 37) % 256).unwrap())
        .collect();
    p.memory.write(u64::from(source), &bytes).unwrap();
    p.memory
        .protect(0x2000_0000, PAGE_SIZE * 18, Permissions::READ)
        .unwrap();
    p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for size in [0, 1, 255, 256, 4095, 4096, 4097, 65537] {
        p.memory
            .write(u64::from(destination - 1), &vec![0x55; size + 2])
            .unwrap();
        p.memory
            .protect(
                0x3000_0000,
                PAGE_SIZE * 18,
                Permissions {
                    read: false,
                    write: true,
                    execute: false,
                },
            )
            .unwrap();
        let before = prepare(&mut p, destination, source, u32::try_from(size).unwrap());
        success(&mut p, before, destination, 0x0040_1000);
        p.memory
            .protect(0x3000_0000, PAGE_SIZE * 18, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(read(&p, destination, size), bytes[..size]);
        assert_eq!(read(&p, destination - 1, 1), [0x55]);
        assert_eq!(
            read(&p, destination + u32::try_from(size).unwrap(), 1),
            [0x55]
        );
    }
    assert_eq!(read(&p, source, bytes.len()), bytes);
    assert_eq!(read(&p, 0x7000_2020, 4), 123_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn full_source_destination_and_frame_checks_prevent_partial_copy() {
    let mut p = process();
    p.memory.write(u64::from(SOURCE), &[1; 16]).unwrap();
    p.memory.write(u64::from(DESTINATION), &[0x55; 16]).unwrap();
    p.memory.write(0x0040_2fff, &[0x66]).unwrap();
    for (destination, source, count) in [
        (DESTINATION, 0x0040_2fff, 2),
        (0x0040_2fff, SOURCE, 2),
        (DESTINATION, 0, 1),
        (0, SOURCE, 1),
        (DESTINATION, u32::MAX, 2),
        (u32::MAX, SOURCE, 2),
        (DESTINATION, SOURCE, u32::MAX),
    ] {
        let before = prepare(&mut p, destination, source, count);
        let result = p.run(1);
        assert!(matches!(
            result.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, DESTINATION, 16), [0x55; 16]);
        assert_eq!(read(&p, 0x0040_2fff, 1), [0x66]);
    }
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, DESTINATION, SOURCE, 16);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(read(&p, DESTINATION, 16), [0x55; 16]);
    prepare(&mut p, DESTINATION, SOURCE, 0);
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn overlapping_spans_refuse_but_adjacent_and_empty_spans_are_valid() {
    let mut p = process();
    p.memory.write(u64::from(SOURCE), b"abcdefgh").unwrap();
    for (destination, source) in [(SOURCE, SOURCE), (SOURCE + 1, SOURCE), (SOURCE, SOURCE + 1)] {
        let before = prepare(&mut p, destination, source, 4);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, SOURCE, 8), b"abcdefgh");
    }
    let before = prepare(&mut p, SOURCE + 4, SOURCE, 4);
    success(&mut p, before, SOURCE + 4, 0x0040_1000);
    assert_eq!(read(&p, SOURCE, 8), b"abcdabcd");
    for (destination, source) in [(0, u32::MAX), (u32::MAX, 0), (0x7000_0000, 0x7000_0000)] {
        let before = prepare(&mut p, destination, source, 0);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, destination, 0x0040_1000);
    }
}

#[test]
fn top_byte_and_captured_frame_or_error_cell_aliases_follow_normal_dispatch() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .write(u64::from(SOURCE), &0x0040_1080_u32.to_le_bytes())
        .unwrap();
    for (destination, source, count, resume) in [
        (u32::MAX, SOURCE, 1, 0x0040_1000),
        (DESTINATION, u32::MAX, 1, 0x0040_1000),
        (STACK + 4, SOURCE, 4, 0x0040_1000),
        (STACK, SOURCE, 4, 0x0040_1080),
        (0x7ffd_e034, SOURCE, 4, 0x0040_1000),
        (0x7000_2020, SOURCE, 4, 0x0040_1000),
    ] {
        let before = prepare(&mut p, destination, source, count);
        let bytes = read(&p, source, count as usize);
        success(&mut p, before, destination, resume);
        assert_eq!(read(&p, destination, count as usize), bytes);
    }
}
