use super::imported_executable;

use super::windows_string_length_executable;

use ring3_core::execution::{
    PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_028c;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2180;

#[test]
fn imported_guest_lengths_agree_whole_and_single_step() {
    for budget in [1, 50] {
        let mut process = Process32::load(&windows_string_length_executable::pe32(), 32).unwrap();
        let mut counts = (0, 0);
        loop {
            let result = process.run(budget);
            counts.0 += result.instructions;
            counts.1 += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(counts.0 + counts.1 < 50);
        }
        assert_eq!(counts, (9, 3));
        assert_eq!(process.cpu.register(Register32::Eax), 0);
        assert_eq!(process.cpu.register(Register32::Ebx), 3);
        assert_eq!(process.cpu.register(Register32::Ecx), 2);
        assert_eq!(process.cpu.register(Register32::Esp), 0x1001_0000);
    }
}

fn process() -> Process32 {
    Process32::load(
        &imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["lstrlenA"]),
        64,
    )
    .unwrap()
}

fn prepare(process: &mut Process32, source: u32) {
    process.cpu.eip = API;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 99);
    process.cpu.eflags = 0xced7;
    for (index, value) in [0x0040_1000_u32, source].into_iter().enumerate() {
        process
            .memory
            .write(u64::from(STACK) + index as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn length(process: &mut Process32, source: u32, expected_length: u32) {
    prepare(process, source);
    let mut expected = process.cpu;
    let pages = process.memory.mapped_pages();
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + 8);
    expected.set_register(Register32::Eax, expected_length);
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
fn counts_bytes_without_mutating_strings_registers_or_error_cells() {
    let mut process = process();
    process
        .memory
        .write(u64::from(SOURCE), b"a\x80\xff\0tail")
        .unwrap();
    process
        .memory
        .write(0x7000_2020, &123_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .write(0x7ffd_e034, &77_u32.to_le_bytes())
        .unwrap();
    process
        .memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    for (offset, expected) in [(0, 3), (1, 2), (2, 1), (3, 0), (4, 4)] {
        length(&mut process, SOURCE + offset, expected);
    }
    let mut source = [0; 8];
    process.memory.read(u64::from(SOURCE), &mut source).unwrap();
    assert_eq!(&source, b"a\x80\xff\0tail");
    let mut errno = [0; 4];
    process.memory.read(0x7000_2020, &mut errno).unwrap();
    assert_eq!(u32::from_le_bytes(errno), 123);
    assert_eq!(process.last_error().unwrap(), 77);
    length(&mut process, STACK + 4, 2);
}

#[test]
fn stops_at_page_edge_nul_and_reads_across_pages_only_when_needed() {
    let mut process = process();
    process.memory.write(0x0040_2ffe, b"x\0").unwrap();
    length(&mut process, 0x0040_2ffe, 1);
    process.memory.write(0x0040_2fff, b"y").unwrap();
    prepare(&mut process, 0x0040_2ffe);
    fault(&mut process);
    process
        .memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0x0040_3000, b"z\0").unwrap();
    length(&mut process, 0x0040_2ffe, 3);
    process
        .memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    prepare(&mut process, 0x0040_2ffe);
    fault(&mut process);
}

#[test]
fn invalid_sources_frames_and_zero_budget_preserve_process_state() {
    let mut process = process();
    for source in [0x6000_0000, 0x7000_0000] {
        prepare(&mut process, source);
        let before = process.cpu;
        assert_eq!(process.run(0).api_calls, 0);
        assert_eq!(process.cpu, before);
        fault(&mut process);
    }
    prepare(&mut process, SOURCE);
    process.cpu.set_register(Register32::Esp, 0x1000_fffc);
    fault(&mut process);
    process
        .memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    length(&mut process, u32::MAX, 0);
    process.memory.write(u64::from(u32::MAX), b"x").unwrap();
    prepare(&mut process, u32::MAX);
    fault(&mut process);
}

#[test]
fn scan_budget_counts_nul_and_never_returns_a_truncated_length() {
    for source in [0x2000_0000, 0xffff_0000] {
        let mut process = process();
        process
            .memory
            .map_zeroed(u64::from(source), PAGE_SIZE * 16, Permissions::READ_WRITE)
            .unwrap();
        process
            .memory
            .write(u64::from(source), &vec![b'x'; 65536])
            .unwrap();
        prepare(&mut process, source);
        let before = process.cpu;
        let result = process.run(1);
        assert_eq!(result.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((result.instructions, result.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        process
            .memory
            .write(u64::from(source) + 65535, &[0])
            .unwrap();
        length(&mut process, source, 65535);
    }
}

#[test]
fn null_never_reads_guest_address_zero_or_error_cells() {
    let mut process = process();
    process
        .memory
        .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    process.memory.write(0, b"not empty\0").unwrap();
    for page in [0, 0x7000_2000, 0x7ffd_e000] {
        process
            .memory
            .protect(page, PAGE_SIZE, Permissions::NONE)
            .unwrap();
    }
    length(&mut process, 0, 0);
    process.memory.write(u64::from(SOURCE), b"abc\0").unwrap();
    length(&mut process, SOURCE, 3);
}
