#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_01b0;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DESTINATION: u32 = 0x0040_2800;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["memmove"]),
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
fn empty_moves_return_destination_without_accessing_either_span() {
    let mut p = process();
    for (destination, source) in [(0, 0), (u32::MAX, 0), (0, u32::MAX), (u32::MAX, u32::MAX)] {
        let before = prepare(&mut p, destination, source, 0);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, destination, 0x0040_1000);
    }
}

#[test]
fn overlapping_moves_preserve_the_original_source_in_both_directions() {
    let mut p = process();
    for (destination, source, expected) in [
        (SOURCE + 2, SOURCE, b"ababcdefij".as_slice()),
        (SOURCE, SOURCE + 2, b"cdefghghij".as_slice()),
        (SOURCE, SOURCE, b"abcdefghij".as_slice()),
    ] {
        p.memory.write(u64::from(SOURCE), b"abcdefghij").unwrap();
        let before = prepare(&mut p, destination, source, 6);
        success(&mut p, before, destination, 0x0040_1000);
        assert_eq!(read(&p, SOURCE, 10), expected);
    }
}

#[test]
fn large_overlaps_cross_chunks_and_pages_in_either_direction() {
    let mut p = process();
    let base: u32 = 0x2000_0000;
    let length = 12_289_usize;
    p.memory
        .map_zeroed(u64::from(base), PAGE_SIZE * 5, Permissions::READ_WRITE)
        .unwrap();
    let original: Vec<_> = (0..length)
        .map(|index| u8::try_from((index * 37) % 251).unwrap())
        .collect();
    for (destination, source, count) in [
        (base + 1, base, length - 1),
        (base, base + 1, length - 1),
        (base + 4097, base, 8192),
    ] {
        p.memory.write(u64::from(base), &original).unwrap();
        let mut expected = original.clone();
        let source_offset = usize::try_from(source - base).unwrap();
        let destination_offset = usize::try_from(destination - base).unwrap();
        expected.copy_within(source_offset..source_offset + count, destination_offset);
        let before = prepare(&mut p, destination, source, u32::try_from(count).unwrap());
        success(&mut p, before, destination, 0x0040_1000);
        assert_eq!(read(&p, base, length), expected);
    }
}

#[test]
fn nonoverlapping_move_accepts_read_only_source_and_write_only_destination() {
    let mut p = process();
    for base in [0x2000_0000, 0x3000_0000] {
        p.memory
            .map_zeroed(base, PAGE_SIZE * 2, Permissions::READ_WRITE)
            .unwrap();
    }
    let source = 0x2000_0ffd;
    let destination = 0x3000_0001;
    let bytes: Vec<_> = (0..4097)
        .map(|index| u8::try_from(index % 256).unwrap())
        .collect();
    p.memory.write(u64::from(source), &bytes).unwrap();
    p.memory
        .protect(0x2000_0000, PAGE_SIZE * 2, Permissions::READ)
        .unwrap();
    p.memory
        .protect(
            0x3000_0000,
            PAGE_SIZE * 2,
            Permissions {
                read: false,
                write: true,
                execute: false,
            },
        )
        .unwrap();
    let before = prepare(&mut p, destination, source, 4097);
    success(&mut p, before, destination, 0x0040_1000);
    p.memory
        .protect(0x3000_0000, PAGE_SIZE * 2, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(read(&p, destination, bytes.len()), bytes);
}

#[test]
fn complete_span_checks_prevent_partial_moves() {
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
}

#[test]
fn captured_frames_and_top_byte_follow_normal_dispatch() {
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
    ] {
        let before = prepare(&mut p, destination, source, count);
        let bytes = read(&p, source, count as usize);
        success(&mut p, before, destination, resume);
        assert_eq!(read(&p, destination, count as usize), bytes);
    }
}
