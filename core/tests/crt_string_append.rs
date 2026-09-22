#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/string_append_executable.rs"]
mod string_append_executable;

use ring3_core::execution::{Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason};

const API: u32 = 0x7000_018c;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;
const DESTINATION: u32 = 0x0040_2800;

#[test]
fn imported_append_terminates_a_prefix_and_leaves_cleanup_to_the_caller() {
    for budget in [1, 50] {
        let mut p = Process32::load(&string_append_executable::pe32(), 32).unwrap();
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
        let mut bytes = [0; 8];
        p.memory.read(0x0040_2190, &mut bytes).unwrap();
        assert_eq!(bytes, [b'Q', b'a', 0x80, 0, 0x55, 0x55, 0x55, 0x55]);
    }
}

fn process() -> Process32 {
    Process32::load(&string_append_executable::pe32(), 128).unwrap()
}

fn prepare(p: &mut Process32, destination: u32, source: u32, count: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, value) in [0x0040_1000, destination, source, count]
        .into_iter()
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn read(p: &Process32, address: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn call(p: &mut Process32, destination: u32, source: u32, count: u32) {
    let mut expected = prepare(p, destination, source, count);
    let pages = p.memory.mapped_pages();
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, destination);
    assert_eq!(p.cpu, expected);
    assert_eq!(p.memory.mapped_pages(), pages);
}

fn stopped(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
    let pages = p.memory.mapped_pages();
    let run = p.run(1);
    if unsupported {
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
    } else {
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
    }
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
}

#[test]
fn byte_oracle_covers_early_nul_truncation_and_page_chunk_boundaries() {
    let mut p = process();
    let (source, destination) = (0x2000_0ffd, 0x3000_0001);
    for base in [0x2000_0000, 0x3000_0000] {
        p.memory
            .map_zeroed(base, 4096 * 18, Permissions::READ_WRITE)
            .unwrap();
    }
    for nul in [0, 3, 4096, 65536] {
        let mut bytes: Vec<_> = (0..65537)
            .map(|i| u8::try_from(i % 255 + 1).unwrap())
            .collect();
        bytes[nul] = 0;
        p.memory.write(u64::from(source), &bytes).unwrap();
        for count in [0, 1, 3, 4097, 65536] {
            let mut expected = vec![0x55; 65540];
            expected[..3].copy_from_slice(b"Hi\0");
            p.memory.write(u64::from(destination), &expected).unwrap();
            let suffix: Vec<_> = bytes
                .iter()
                .copied()
                .take(count as usize)
                .take_while(|&b| b != 0)
                .collect();
            expected[2..2 + suffix.len()].copy_from_slice(&suffix);
            expected[2 + suffix.len()] = 0;
            call(&mut p, destination, source, count);
            assert_eq!(read(&p, destination, expected.len()), expected);
            assert_eq!(read(&p, source, bytes.len()), bytes);
        }
    }
}

#[test]
fn source_reads_stop_at_count_or_nul_without_touching_the_next_page() {
    let mut p = process();
    p.memory.write(0x0040_2ffe, b"xy").unwrap();
    p.memory.write(u64::from(DESTINATION), b"Q\0").unwrap();
    call(&mut p, DESTINATION, 0x0040_2ffe, 2);
    assert_eq!(read(&p, DESTINATION, 4), b"Qxy\0");
    prepare(&mut p, DESTINATION, 0x0040_2ffe, 3);
    stopped(&mut p, false);
    assert_eq!(read(&p, DESTINATION, 4), b"Qxy\0");
    p.memory.write(0x0040_2fff, &[0]).unwrap();
    call(&mut p, DESTINATION, 0x0040_2ffe, 65536);
    assert_eq!(read(&p, DESTINATION, 5), b"Qxyx\0");
}

#[test]
fn the_entire_append_including_nul_is_writable_before_any_output_changes() {
    let mut p = process();
    p.memory.write(0x0040_2ffe, b"Q\0").unwrap();
    prepare(&mut p, 0x0040_2ffe, SOURCE, 2);
    stopped(&mut p, false);
    assert_eq!(read(&p, 0x0040_2ffe, 2), b"Q\0");
    p.memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, 0x0040_2ffe, SOURCE, 2);
    stopped(&mut p, false);
    assert_eq!(read(&p, 0x0040_2ffe, 2), b"Q\0");
    p.memory
        .protect(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, 0x0040_2ffe, SOURCE, 2);
    assert_eq!(read(&p, 0x0040_2ffe, 4), b"Qa\x80\0");
}

#[test]
fn overlapping_source_and_destination_strings_are_rejected_without_writes() {
    let mut p = process();
    for (dst, src, count) in [(0, 0, 1), (0, 1, 1), (0, 4, 1), (0, 5, 4), (2, 0, 4)] {
        p.memory
            .write(u64::from(DESTINATION), b"abcd\0efgh\0tail")
            .unwrap();
        prepare(&mut p, DESTINATION + dst, DESTINATION + src, count);
        stopped(&mut p, true);
        assert_eq!(read(&p, DESTINATION, 14), b"abcd\0efgh\0tail");
    }
}

#[test]
fn zero_count_needs_no_pointers_and_scan_limits_are_explicit() {
    let mut p = process();
    call(&mut p, u32::MAX, 0, 0);
    prepare(&mut p, 0, 0, 65537);
    stopped(&mut p, true);
    p.memory
        .map_zeroed(0x2000_0000, 65536, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x2000_0000, &vec![b'x'; 65536]).unwrap();
    prepare(&mut p, 0x2000_0000, SOURCE, 1);
    stopped(&mut p, true);
    p.memory.write(0x2000_ffff, &[0]).unwrap();
    call(&mut p, 0x2000_0000, SOURCE + 3, 1);
    assert_eq!(read(&p, 0x2000_fffe, 2), b"x\0");
}

#[test]
fn checked_32bit_edges_do_not_wrap_source_or_output() {
    let mut p = process();
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, u32::MAX, SOURCE + 3, 1);
    prepare(&mut p, u32::MAX, SOURCE, 1);
    stopped(&mut p, false);
    assert_eq!(read(&p, u32::MAX, 1), [0]);
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    prepare(&mut p, DESTINATION, u32::MAX, 2);
    stopped(&mut p, false);
    assert_eq!(read(&p, DESTINATION, 2), [0, 0]);
    call(&mut p, DESTINATION, u32::MAX, 1);
    assert_eq!(read(&p, DESTINATION, 2), b"x\0");
}

#[test]
fn error_cells_are_untouched_and_invalid_frames_or_zero_budget_do_not_append() {
    let mut p = process();
    for page in [0x7000_2000, 0x7ffd_e000] {
        p.memory.protect(page, 4096, Permissions::NONE).unwrap();
    }
    call(&mut p, DESTINATION, SOURCE, 2);
    assert_eq!(read(&p, DESTINATION, 3), b"a\x80\0");
    let cpu = prepare(&mut p, DESTINATION, SOURCE, 2);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    assert_eq!(read(&p, DESTINATION, 3), b"a\x80\0");
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    stopped(&mut p, false);
    assert_eq!(read(&p, DESTINATION, 3), b"a\x80\0");
    prepare(&mut p, DESTINATION, 0, 2);
    stopped(&mut p, false);
    assert_eq!(read(&p, DESTINATION, 3), b"a\x80\0");
}
