use super::file_status_executable;
use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

#[test]
fn imported_legacy_stat_serializes_the_default_root_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = Process32::load(&file_status_executable::pe32(), 64).unwrap();
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
        assert_eq!(counts, (5, 1));
        assert_eq!(p.cpu.register(Register32::Eax), 0);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 36];
        p.memory.read(0x0040_2280, &mut bytes).unwrap();
        let mut expected = [0; 36];
        expected[0] = 2;
        expected[16] = 2;
        expected[6..8].copy_from_slice(&0x41ff_u16.to_le_bytes());
        expected[8] = 1;
        assert_eq!(bytes, expected);
    }
}

use ring3_core::execution::{Cpu32, FileMetadata, PAGE_SIZE, Permissions, ProcessOptions};
const API: u32 = 0x7000_015c;
const STACK: u32 = 0x1000_ff00;
const SOURCE: u32 = 0x0040_2400;
const OUTPUT: u32 = 0x0040_2600;
const ERRNO: u64 = 0x7000_2020;

fn load() -> Process32 {
    Process32::load_with_options(
        &file_status_executable::pe32(),
        64,
        ProcessOptions {
            current_directory: b"D:\\Base",
            files: &[
                FileMetadata {
                    path: b"D:\\Base\\inner\\data.bin",
                    size: 42,
                },
                FileMetadata {
                    path: b"D:\\Base\\run.ExE",
                    size: 7,
                },
                FileMetadata {
                    path: b"D:\\Base\\run.bat",
                    size: 8,
                },
                FileMetadata {
                    path: b"D:\\Base\\run.cmd",
                    size: 9,
                },
                FileMetadata {
                    path: b"D:\\Base\\run.com",
                    size: 10,
                },
                FileMetadata {
                    path: b"D:\\Base\\limit",
                    size: 0x7fff_ffff,
                },
                FileMetadata {
                    path: b"D:\\Base\\large",
                    size: 0x8000_0000,
                },
            ],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn prepare(p: &mut Process32, source: u32, output: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    for (i, word) in [0x0040_1000_u32, source, output].iter().enumerate() {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &word.to_le_bytes())
            .unwrap();
    }
    p.cpu
}

fn finish(p: &mut Process32, mut expected: Cpu32, value: u32) {
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, expected);
    let run = p.run(1);
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    let mut resume = [0; 4];
    p.memory.read(u64::from(STACK), &mut resume).unwrap();
    expected.eip = u32::from_le_bytes(resume);
    expected.set_register(Register32::Esp, STACK + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

fn query(p: &mut Process32, path: &[u8], output: u32, expected: u32) {
    p.memory.write(u64::from(SOURCE), path).unwrap();
    p.memory
        .write(u64::from(SOURCE) + path.len() as u64, &[0])
        .unwrap();
    let before = prepare(p, SOURCE, output);
    finish(p, before, expected);
}

fn word(p: &Process32, address: u64) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(address, &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn record(p: &Process32, output: u32, drive: u8, mode: u16, size: u32) {
    let mut expected = [0; 36];
    expected[0] = drive;
    expected[16] = drive;
    expected[8] = 1;
    expected[6..8].copy_from_slice(&mode.to_le_bytes());
    expected[20..24].copy_from_slice(&size.to_le_bytes());
    let mut actual = [0; 36];
    p.memory.read(u64::from(output), &mut actual).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn shared_catalog_reports_files_implicit_directories_drive_modes_and_sizes() {
    let mut p = load();
    p.memory.write(ERRNO, &123_u32.to_le_bytes()).unwrap();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for (path, mode, size) in [
        (&b"."[..], 0x41ff, 0),
        (b"..", 0x41ff, 0),
        (b"\\", 0x41ff, 0),
        (b"d:\\", 0x41ff, 0),
        (b"inner", 0x41ff, 0),
        (b"INNER\\data.BIN", 0x81b6, 42),
        (b"inner\\..\\run.exe", 0x81ff, 7),
        (b"run.bat", 0x81ff, 8),
        (b"run.CMD", 0x81ff, 9),
        (b"run.com", 0x81ff, 10),
        (b"limit", 0x81b6, 0x7fff_ffff),
    ] {
        query(&mut p, path, OUTPUT, 0);
        record(&p, OUTPUT, 3, mode, size);
        assert_eq!(word(&p, ERRNO), 123);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    let mut other = Process32::load(&file_status_executable::pe32(), 64).unwrap();
    query(&mut other, b"D:\\Base", 0, u32::MAX);
    assert_eq!(word(&other, ERRNO), 2);
}

#[test]
fn absent_invalid_and_legacy_trailing_separator_paths_only_write_errno() {
    let mut p = load();
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for path in [
        &b""[..],
        b"absent",
        b"*",
        b"C:\\",
        b"inner\\",
        b"D:\\Base\\",
        b"inner\\data.bin\\",
        b"..\\",
    ] {
        p.memory.write(u64::from(OUTPUT), &[0x55; 36]).unwrap();
        query(&mut p, path, OUTPUT, u32::MAX);
        let mut output = [0; 36];
        p.memory.read(u64::from(OUTPUT), &mut output).unwrap();
        assert_eq!(output, [0x55; 36]);
        assert_eq!(word(&p, ERRNO), 2);
        assert_eq!(p.last_error().unwrap(), 77);
    }
    query(&mut p, b"absent", 0, u32::MAX);
    query(&mut p, b"absent", u32::try_from(ERRNO).unwrap(), u32::MAX);
    assert_eq!(word(&p, ERRNO), 2);
}

#[test]
fn unsupported_paths_and_sizes_leave_cpu_errors_and_output_unchanged() {
    let mut p = load();
    for path in [
        &b"large"[..],
        b"D:Base",
        b"\\\\host\\share\\",
        b"NUL\\",
        b"a\xff\\",
        b"D:/Base",
        b"trail.",
    ] {
        p.memory.write(u64::from(SOURCE), path).unwrap();
        p.memory
            .write(u64::from(SOURCE) + path.len() as u64, &[0])
            .unwrap();
        p.memory.write(ERRNO, &123_u32.to_le_bytes()).unwrap();
        p.memory.write(u64::from(OUTPUT), &[0x55; 36]).unwrap();
        let before = prepare(&mut p, SOURCE, OUTPUT);
        let run = p.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!(run.api_calls, 0);
        assert_eq!(p.cpu, before);
        assert_eq!(word(&p, ERRNO), 123);
        assert_eq!(word(&p, u64::from(OUTPUT)), 0x5555_5555);
    }
}

#[test]
fn output_faults_error_faults_and_read_only_error_cells_are_atomic() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b".\0").unwrap();
    for (source, output) in [
        (SOURCE, 0),
        (SOURCE, 0x0040_2ff0),
        (SOURCE, u32::MAX - 34),
        (0, OUTPUT),
    ] {
        let before = prepare(&mut p, source, output);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(run.api_calls, 0);
        assert_eq!(p.cpu, before);
    }
    p.memory
        .protect(0x7000_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    query(&mut p, b".", OUTPUT, 0);
    record(&p, OUTPUT, 3, 0x41ff, 0);
    p.memory.write(u64::from(SOURCE), b"absent\0").unwrap();
    let before = prepare(&mut p, SOURCE, OUTPUT);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    record(&p, OUTPUT, 3, 0x41ff, 0);
    p.memory.write(u64::from(SOURCE), b".\0").unwrap();
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, SOURCE, OUTPUT);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn captured_inputs_and_full_width_outputs_allow_aliases_and_top_address() {
    let mut p = load();
    query(&mut p, b"inner\\data.bin", SOURCE, 0);
    record(&p, SOURCE, 3, 0x81b6, 42);
    query(&mut p, b".", STACK, 0);
    assert_eq!(p.cpu.eip, 3);
    record(&p, STACK, 3, 0x41ff, 0);
    query(&mut p, b".", STACK + 4, 0);
    record(&p, STACK + 4, 3, 0x41ff, 0);
    query(&mut p, b".", u32::try_from(ERRNO).unwrap(), 0);
    assert_eq!(word(&p, ERRNO), 3);
    p.memory.write(ERRNO, b"missing\0").unwrap();
    let before = prepare(&mut p, u32::try_from(ERRNO).unwrap(), OUTPUT);
    finish(&mut p, before, u32::MAX);
    assert_eq!(word(&p, ERRNO), 2);
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, b".\0").unwrap();
    let before = prepare(&mut p, 0xffff_fffe, u32::MAX - 35);
    finish(&mut p, before, 0);
    record(&p, u32::MAX - 35, 3, 0x41ff, 0);
    p.memory.write(u64::from(u32::MAX), b".").unwrap();
    let before = prepare(&mut p, u32::MAX, OUTPUT);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}
