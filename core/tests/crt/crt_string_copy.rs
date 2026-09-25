use super::imported_executable;
use super::string_copy_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

#[test]
fn imported_copy_pads_and_leaves_argument_cleanup_to_the_caller() {
    for budget in [1, 50] {
        let mut p = Process32::load(&string_copy_executable::pe32(), 32).unwrap();
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
        assert_eq!(counts, (6, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 0x0040_2190);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 10];
        p.memory.read(0x0040_2190, &mut bytes).unwrap();
        assert_eq!(bytes, [b'a', 0x80, b'b', 0, 0, 0, 0, 0, 0x55, 0x55]);
    }
}

const API: u32 = 0x7000_0154;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DESTINATION: u32 = 0x0040_2800;

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["strncpy"]),
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
fn truncation_and_padding_match_an_independent_byte_oracle_across_chunks() {
    let mut p = process();
    let source = 0x2000_0ffd;
    let destination = 0x3000_0001;
    for base in [0x2000_0000, 0x3000_0000] {
        p.memory
            .map_zeroed(base, PAGE_SIZE * 18, Permissions::READ_WRITE)
            .unwrap();
    }
    p.memory.write(0x7000_2020, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for nul in [0, 3, 4096, 65537] {
        let mut source_bytes: Vec<_> = (0..65538)
            .map(|i| u8::try_from(i % 255 + 1).unwrap())
            .collect();
        source_bytes[nul] = 0;
        p.memory
            .protect(0x2000_0000, PAGE_SIZE * 18, Permissions::READ_WRITE)
            .unwrap();
        p.memory.write(u64::from(source), &source_bytes).unwrap();
        p.memory
            .protect(0x2000_0000, PAGE_SIZE * 18, Permissions::READ)
            .unwrap();
        for size in [0, 1, 3, 4, 4095, 4096, 4097, 65538] {
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
            let mut expected = vec![0; size];
            for (output, &input) in expected
                .iter_mut()
                .zip(source_bytes.iter().take_while(|&&b| b != 0))
            {
                *output = input;
            }
            assert_eq!(read(&p, destination, size), expected);
            assert_eq!(read(&p, destination - 1, 1), [0x55]);
            assert_eq!(
                read(&p, destination + u32::try_from(size).unwrap(), 1),
                [0x55]
            );
        }
        assert_eq!(read(&p, source, source_bytes.len()), source_bytes);
    }
    assert_eq!(read(&p, 0x7000_2020, 4), 123_u32.to_le_bytes());
    assert_eq!(p.last_error().unwrap(), 77);
}

#[test]
fn source_reads_stop_at_nul_or_count_including_the_top_address() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for source in [0x0040_2fff, u32::MAX] {
        p.memory.write(u64::from(source), &[0]).unwrap();
        let before = prepare(&mut p, DESTINATION, source, 8);
        success(&mut p, before, DESTINATION, 0x0040_1000);
        assert_eq!(read(&p, DESTINATION, 8), [0; 8]);
        p.memory.write(u64::from(source), &[0x80]).unwrap();
        let before = prepare(&mut p, DESTINATION, source, 1);
        success(&mut p, before, DESTINATION, 0x0040_1000);
        assert_eq!(read(&p, DESTINATION, 1), [0x80]);
    }
    p.memory.write(u64::from(SOURCE), b"a\0").unwrap();
    let before = prepare(&mut p, u32::MAX, SOURCE, 1);
    success(&mut p, before, u32::MAX, 0x0040_1000);
    assert_eq!(read(&p, u32::MAX, 1), b"a");
    let before = prepare(&mut p, u32::MAX - 1, SOURCE, 2);
    success(&mut p, before, u32::MAX - 1, 0x0040_1000);
    assert_eq!(read(&p, u32::MAX - 1, 2), b"a\0");
}

#[test]
fn inaccessible_source_destination_and_frame_leave_everything_unchanged() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(u64::from(SOURCE), b"ab\0").unwrap();
    p.memory.write(u64::from(DESTINATION), &[0x55; 16]).unwrap();
    for source in [0x0040_2fff, u32::MAX] {
        p.memory.write(u64::from(source), &[0x66]).unwrap();
    }
    for (destination, source, count) in [
        (DESTINATION, 0x0040_2fff, 2),
        (DESTINATION, u32::MAX, 2),
        (0x0040_2fff, SOURCE, 2),
        (u32::MAX, SOURCE, 2),
        (DESTINATION, 0, 1),
        (0, SOURCE, 1),
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
        assert_eq!(read(&p, u32::MAX, 1), [0x66]);
    }
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, DESTINATION, SOURCE, 8);
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
fn actual_read_span_including_nul_controls_overlap_rejection() {
    let mut p = process();
    p.memory.write(u64::from(SOURCE), b"abc\0efgh").unwrap();
    for (destination, source, count) in [
        (SOURCE, SOURCE, 4),
        (SOURCE + 3, SOURCE, 4),
        (SOURCE, SOURCE + 3, 4),
    ] {
        let before = prepare(&mut p, destination, source, count);
        let result = p.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(p.cpu, before);
        assert_eq!(read(&p, SOURCE, 8), b"abc\0efgh");
    }
    // the count-sized hypothetical source span overlaps, but no byte after its NUL is read.
    let before = prepare(&mut p, SOURCE + 4, SOURCE, 8);
    success(&mut p, before, SOURCE + 4, 0x0040_1000);
    assert_eq!(read(&p, SOURCE + 4, 8), b"abc\0\0\0\0\0");
    for (destination, source) in [(0, u32::MAX), (u32::MAX, 0), (SOURCE, SOURCE)] {
        let before = prepare(&mut p, destination, source, 0);
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, before);
        success(&mut p, before, destination, 0x0040_1000);
    }
}

#[test]
fn captured_arguments_return_slot_and_error_cell_aliases_follow_dispatch() {
    let mut p = process();
    p.memory
        .write(u64::from(SOURCE), &0x0040_1080_u32.to_le_bytes())
        .unwrap();
    for (destination, resume) in [
        (STACK + 4, 0x0040_1000),
        (STACK, 0x0040_1080),
        (0x7ffd_e034, 0x0040_1000),
        (0x7000_2020, 0x0040_1000),
    ] {
        let before = prepare(&mut p, destination, SOURCE, 4);
        success(&mut p, before, destination, resume);
        assert_eq!(read(&p, destination, 4), 0x0040_1080_u32.to_le_bytes());
    }
}
