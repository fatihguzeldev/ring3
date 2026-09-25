use super::file_attributes_executable;

use ring3_core::execution::{
    FileMetadata, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0290;
const SOURCE: u32 = 0x0040_2400;
const STACK: u32 = 0x1000_ff00;

fn load(files: &[FileMetadata<'_>]) -> Process32 {
    Process32::load_with_options(
        &file_attributes_executable::pe32(),
        64,
        ProcessOptions {
            files,
            directories: &[b"D:\\empty"],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

#[test]
fn imported_attributes_agree_whole_and_stepwise_with_the_process_catalog() {
    let files = [FileMetadata {
        path: b"C:\\sample.bin",
        size: 42,
    }];
    for (files, value) in [(&[][..], u32::MAX), (&files[..], 128)] {
        for budget in [1, 50] {
            let mut p = load(files);
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 50);
            }
            assert_eq!(counts, (6, 2));
            assert_eq!(p.cpu.register(Register32::Ebx), 16);
            assert_eq!(p.cpu.register(Register32::Eax), value);
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        }
    }
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) {
    p.cpu.eip = api;
    p.cpu.eflags = 0xced7;
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Esp, STACK);
    for (i, value) in std::iter::once(0x0040_1000)
        .chain(args.iter().copied())
        .enumerate()
    {
        p.memory
            .write(u64::from(STACK) + i as u64 * 4, &value.to_le_bytes())
            .unwrap();
    }
}

fn call(p: &mut Process32, api: u32, args: &[u32], value: u32) {
    prepare(p, api, args);
    let mut expected = p.cpu;
    let pages = p.memory.mapped_pages();
    let cleanup = if api == 0x7000_0188 {
        4
    } else {
        u32::try_from(args.len() + 1).unwrap() * 4
    };
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, STACK + cleanup);
    expected.set_register(Register32::Eax, value);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    assert_eq!(p.cpu, expected);
    assert_eq!(p.memory.mapped_pages(), pages);
}

fn query(p: &mut Process32, path: &[u8], expected: u32) {
    p.memory.write(u64::from(SOURCE), path).unwrap();
    call(p, API, &[SOURCE], expected);
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
fn normalized_file_and_directory_queries_follow_current_directory_and_search_metadata() {
    let mut p = load(&[FileMetadata {
        path: b"C:\\Folder\\sample.bin",
        size: 42,
    }]);
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    for path in [
        b"C:\\\\Folder\\SAMPLE.bin\0".as_slice(),
        b"folder\\..\\folder\\sample.bin\0",
        b"\\FOLDER\\sample.bin\0",
    ] {
        query(&mut p, path, 128);
    }
    for path in [
        b"C:\\\0".as_slice(),
        b".\0",
        b"Folder\\\0",
        b"D:\\empty\0",
        b"d:\\\0",
    ] {
        query(&mut p, path, 16);
    }
    assert_eq!(p.last_error().unwrap(), 77);
    query(&mut p, b"Folder\0", 16);
    call(&mut p, 0x7000_0230, &[SOURCE], 1);
    query(&mut p, b"sample.bin\0", 128);
    call(&mut p, 0x7000_0234, &[SOURCE, SOURCE + 512], 0x7300_0004);
    let mut attributes = [0; 4];
    p.memory
        .read(u64::from(SOURCE + 512), &mut attributes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(attributes), 128);
    query(&mut p, b"..\0", 16);
}

#[test]
fn absent_leaf_parent_and_invalid_syntax_have_distinct_last_errors() {
    let mut p = load(&[FileMetadata {
        path: b"C:\\file.bin",
        size: 0,
    }]);
    for (path, error) in [
        (b"missing\0".as_slice(), 2),
        (b"D:\\empty\\missing\0", 2),
        (b"missing\\leaf\0", 3),
        (b"file.bin\\leaf\0", 3),
        (b"Z:\\\0", 3),
        (b"Z:\\leaf\0", 3),
        (b"\0", 3),
        (b"bad*name\0", 123),
        (b"1:\\file\0", 123),
    ] {
        query(&mut p, path, u32::MAX);
        assert_eq!(p.last_error().unwrap(), error, "{path:?}");
    }
}

#[test]
fn removing_a_file_changes_attributes_but_preserves_its_implicit_parent() {
    let files = [FileMetadata {
        path: b"C:\\Folder\\sample.bin",
        size: 42,
    }];
    let mut p = load(&files);
    let mut other = load(&files);
    query(&mut p, b"Folder\\sample.bin\0", 128);
    call(&mut p, 0x7000_0188, &[SOURCE], 0);
    query(&mut p, b"Folder\\sample.bin\0", u32::MAX);
    assert_eq!(p.last_error().unwrap(), 2);
    query(&mut p, b"Folder\0", 16);
    query(&mut other, b"Folder\\sample.bin\0", 128);
}

#[test]
fn successful_queries_ignore_error_cells_and_failed_error_writes_are_atomic() {
    let mut p = load(&[]);
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    p.memory.write(u64::from(SOURCE), b"missing\0").unwrap();
    prepare(&mut p, API, &[SOURCE]);
    stopped(&mut p, false);
    assert_eq!(p.last_error().unwrap(), 77);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory.write(u64::from(SOURCE), b"C:\\\0").unwrap();
    p.memory
        .protect(0x0040_2000, 4096, Permissions::READ)
        .unwrap();
    call(&mut p, API, &[SOURCE], 16);
}

#[test]
fn unsupported_names_and_max_path_bounds_never_guess_attributes() {
    let mut p = load(&[FileMetadata {
        path: b"C:\\sample.bin",
        size: 0,
    }]);
    for path in [
        b"sample.bin\\\0".as_slice(),
        b"\\\\server\\share\0",
        b"C:relative\0",
        b"NUL\0",
        b"\x80\0",
        b"trailing.\0",
    ] {
        p.memory.write(u64::from(SOURCE), path).unwrap();
        prepare(&mut p, API, &[SOURCE]);
        stopped(&mut p, true);
    }
    let mut long = b"C:\\".to_vec();
    long.extend_from_slice(&[b'a'; 250]);
    long.extend_from_slice(b"\\bbbbb\0");
    assert_eq!(long.len(), 260);
    query(&mut p, &long, u32::MAX);
    assert_eq!(p.last_error().unwrap(), 3);
    long.insert(long.len() - 1, b'b');
    p.memory.write(u64::from(SOURCE), &long).unwrap();
    prepare(&mut p, API, &[SOURCE]);
    stopped(&mut p, true);

    let cwd = [&b"C:\\"[..], &[b'a'; 255]].concat();
    let mut p = Process32::load_with_options(
        &file_attributes_executable::pe32(),
        64,
        ProcessOptions {
            current_directory: &cwd,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    p.memory.write(u64::from(SOURCE), b"b\0").unwrap();
    prepare(&mut p, API, &[SOURCE]);
    stopped(&mut p, true);
}

#[test]
fn source_frame_and_budget_faults_leave_the_call_uncommitted() {
    let mut p = load(&[]);
    for source in [0, 0x6000_0000, 0x7000_0000] {
        prepare(&mut p, API, &[source]);
        let cpu = p.cpu;
        assert_eq!(p.run(0).api_calls, 0);
        assert_eq!(p.cpu, cpu);
        stopped(&mut p, false);
    }
    prepare(&mut p, API, &[SOURCE]);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    stopped(&mut p, false);
    p.memory.write(0x0040_2ffc, b"C:\\\0").unwrap();
    call(&mut p, API, &[0x0040_2ffc], 16);
    p.memory.write(0x0040_2fff, b"x").unwrap();
    prepare(&mut p, API, &[0x0040_2ffc]);
    stopped(&mut p, false);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    call(&mut p, API, &[u32::MAX], u32::MAX);
    assert_eq!(p.last_error().unwrap(), 3);
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    prepare(&mut p, API, &[u32::MAX]);
    stopped(&mut p, false);
}
