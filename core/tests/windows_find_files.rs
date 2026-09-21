#[path = "support/find_files_executable.rs"]
mod find_files_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

use ring3_core::execution::{Process32, ProcessOptions, ProcessStop, Register32, StopReason};

#[test]
fn imported_directory_search_has_an_owned_handle_lifecycle() {
    for budget in [1, 50] {
        let mut p = Process32::load_with_options(
            &find_files_executable::pe32(),
            64,
            ProcessOptions {
                directories: &[b"C:\\Data", b"C:\\Other"],
                ..ProcessOptions::default()
            },
        )
        .unwrap();
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
        assert_eq!(counts, (10, 3));
        assert_eq!(p.cpu.register(Register32::Eax), 1);
        assert_eq!(p.cpu.register(Register32::Ebx), 0x7300_0004);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut record = [0; 320];
        p.memory.read(0x0040_2280, &mut record).unwrap();
        assert_eq!(record[0], 0x10);
        assert_eq!(&record[44..50], b"Other\0");
    }
}

use ring3_core::execution::{Cpu32, FileMetadata, LoadError, PAGE_SIZE, Permissions};
const FIRST: u32 = 0x7000_0234;
const NEXT: u32 = 0x7000_0238;
const CLOSE: u32 = 0x7000_023c;
const STACK: u32 = 0x1000_f000;
const SOURCE: u32 = 0x0040_2400;
const OUTPUT: u32 = 0x0040_2600;

fn load(files: &[FileMetadata<'_>], directories: &[&[u8]]) -> Process32 {
    Process32::load_with_options(
        &find_files_executable::pe32(),
        64,
        ProcessOptions {
            files,
            directories,
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn prepare(p: &mut Process32, api: u32, args: &[u32]) -> Cpu32 {
    p.cpu.eip = api;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.set_register(Register32::Ebx, 77);
    p.cpu.eflags = 0xced7;
    p.memory
        .write(u64::from(STACK), &0x0040_1000_u32.to_le_bytes())
        .unwrap();
    for (index, value) in args.iter().enumerate() {
        p.memory
            .write(
                u64::from(STACK) + 4 + index as u64 * 4,
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    p.cpu
}

fn call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = prepare(p, api, args);
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, expected);
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    let mut resume = [0; 4];
    p.memory.read(u64::from(STACK), &mut resume).unwrap();
    expected.eip = u32::from_le_bytes(resume);
    expected.set_register(
        Register32::Esp,
        STACK + 4 + u32::try_from(args.len()).unwrap() * 4,
    );
    expected.set_register(Register32::Eax, p.cpu.register(Register32::Eax));
    assert_eq!(p.cpu, expected);
    p.cpu.register(Register32::Eax)
}

fn first(p: &mut Process32, pattern: &[u8], output: u32) -> u32 {
    p.memory.write(u64::from(SOURCE), pattern).unwrap();
    p.memory
        .write(u64::from(SOURCE) + pattern.len() as u64, &[0])
        .unwrap();
    call(p, FIRST, &[SOURCE, output])
}

fn record(p: &Process32, output: u32, name: &[u8], size: u64, directory: bool) {
    let mut actual = [0; 320];
    p.memory.read(u64::from(output), &mut actual).unwrap();
    let mut expected = [0; 320];
    expected[0] = if directory { 0x10 } else { 0x80 };
    let size = size.to_le_bytes();
    expected[28..32].copy_from_slice(&size[4..]);
    expected[32..36].copy_from_slice(&size[..4]);
    expected[44..44 + name.len()].copy_from_slice(name);
    assert_eq!(actual, expected);
}

#[test]
fn files_are_owned_and_enumerated_with_directories_sizes_and_stable_snapshots() {
    let mut path = b"C:\\alpha.RaS".to_vec();
    let mut p = load(
        &[
            FileMetadata {
                path: &path,
                size: 0x1234_5678_90ab_cdef,
            },
            FileMetadata {
                path: b"C:\\Depot\\nested.bin",
                size: 9,
            },
            FileMetadata {
                path: b"C:\\notes",
                size: 0,
            },
        ],
        &[b"C:\\depot\\Other", b"C:\\Archive.RAS"],
    );
    path.fill(b'?');
    p.memory.write(0x7ffd_e034, &77_u32.to_le_bytes()).unwrap();
    let handle = first(&mut p, b"*.ras", OUTPUT);
    assert_eq!(handle, 0x7300_0004);
    record(&p, OUTPUT, b"alpha.RaS", 0x1234_5678_90ab_cdef, false);
    assert_eq!(p.last_error().unwrap(), 77);
    p.memory.write(u64::from(SOURCE), b"Depot\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0230, &[SOURCE]), 1);
    assert_eq!(call(&mut p, NEXT, &[handle, OUTPUT]), 1);
    record(&p, OUTPUT, b"Archive.RAS", 0, true);
    assert_eq!(call(&mut p, NEXT, &[handle, 0]), 0);
    assert_eq!(p.last_error().unwrap(), 18);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    let handle = first(&mut p, b"\\*", OUTPUT);
    for (i, name) in [&b"alpha.RaS"[..], b"Archive.RAS", b"depot", b"notes"]
        .iter()
        .enumerate()
    {
        if i > 0 {
            assert_eq!(call(&mut p, NEXT, &[handle, OUTPUT]), 1);
        }
        record(
            &p,
            OUTPUT,
            name,
            if i == 0 { 0x1234_5678_90ab_cdef } else { 0 },
            i == 1 || i == 2,
        );
    }
    assert_eq!(call(&mut p, NEXT, &[handle, OUTPUT]), 0);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    let handle = first(&mut p, b"..\\ALPHA.ras", OUTPUT);
    record(&p, OUTPUT, b"alpha.RaS", 0x1234_5678_90ab_cdef, false);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    let mut other = load(&[], &[]);
    assert_eq!(first(&mut other, b"alpha.ras", 0), u32::MAX);
    assert_eq!(other.last_error().unwrap(), 2);
    p.memory
        .write(u64::from(SOURCE), b"C:\\alpha.RaS\0")
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_0230, &[SOURCE]), 0);
}

#[test]
fn failed_writes_preserve_handle_ids_and_cursors_and_aliases_capture_inputs() {
    let mut p = load(&[], &[b"C:\\A", b"C:\\B"]);
    p.memory.write(u64::from(SOURCE), b"*\0").unwrap();
    for output in [0, 0x0040_2ff0, u32::MAX - 318] {
        let before = prepare(&mut p, FIRST, &[SOURCE, output]);
        let run = p.run(1);
        assert!(matches!(
            run.reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(run.api_calls, 0);
        assert_eq!(p.cpu, before);
    }
    let handle = call(&mut p, FIRST, &[SOURCE, SOURCE]);
    assert_eq!(handle, 0x7300_0004);
    record(&p, SOURCE, b"A", 0, true);
    let before = prepare(&mut p, NEXT, &[handle, 0x0040_2ff0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, NEXT, &[handle, STACK]), 1);
    assert_eq!(p.cpu.eip, 0x10);
    record(&p, STACK, b"B", 0, true);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    let handle = first(&mut p, b"*", STACK + 4);
    assert_eq!(handle, 0x7300_0008);
    record(&p, STACK + 4, b"A", 0, true);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0xffff_fffe, b"*\0").unwrap();
    let handle = call(&mut p, FIRST, &[0xffff_fffe, u32::MAX - 319]);
    record(&p, u32::MAX - 319, b"A", 0, true);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
}

#[test]
fn error_paths_preserve_outputs_except_last_error_alias_and_success_ignores_teb() {
    let mut p = load(&[], &[b"C:\\Data"]);
    for (pattern, error) in [
        (&b"absent"[..], 2),
        (b"missing\\*", 3),
        (b"", 123),
        (b"C:\\", 123),
    ] {
        p.memory.write(u64::from(OUTPUT), &[0x55; 320]).unwrap();
        assert_eq!(first(&mut p, pattern, OUTPUT), u32::MAX);
        assert_eq!(p.last_error().unwrap(), error);
        let mut bytes = [0; 320];
        p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        assert_eq!(bytes, [0x55; 320]);
    }
    assert_eq!(first(&mut p, b"absent", 0x7ffd_e034), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 2);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let handle = first(&mut p, b"*", OUTPUT);
    let before = prepare(&mut p, NEXT, &[handle, 0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    p.memory.write(u64::from(SOURCE), b"absent\0").unwrap();
    let before = prepare(&mut p, FIRST, &[SOURCE, 0]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
}

#[test]
fn unsupported_filters_and_guest_input_faults_do_not_open_searches() {
    let mut p = load(&[], &[b"C:\\Data"]);
    for pattern in [
        &b"*.*"[..],
        b"a?",
        b"a*",
        b"C:Data",
        b"\\\\host\\*",
        b"a\xff",
        b"NUL",
        b"foo.",
        b"C:\\*\\alpha.dat",
        b"a?\\*.ras",
    ] {
        p.memory.write(u64::from(SOURCE), pattern).unwrap();
        p.memory
            .write(u64::from(SOURCE) + pattern.len() as u64, &[0])
            .unwrap();
        let before = prepare(&mut p, FIRST, &[SOURCE, OUTPUT]);
        assert_eq!(
            p.run(1).reason,
            ProcessStop::UnsupportedApi { address: FIRST }
        );
        assert_eq!(p.cpu, before);
    }
    for source in [0, 0x0040_2fff] {
        p.memory.write(0x0040_2fff, b"x").unwrap();
        let before = prepare(&mut p, FIRST, &[source, OUTPUT]);
        assert!(matches!(
            p.run(1).reason,
            ProcessStop::Stopped(StopReason::MemoryFault(_))
        ));
        assert_eq!(p.cpu, before);
    }
    let handle = first(&mut p, b"*", OUTPUT);
    assert_eq!(handle, 0x7300_0004);
}

#[test]
fn search_handle_capacity_and_handle_types_are_enforced_without_reuse() {
    let mut p = load(&[], &[b"C:\\Data"]);
    for index in 1..=64 {
        assert_eq!(first(&mut p, b"*", OUTPUT), 0x7300_0000 + index * 4);
    }
    assert_eq!(first(&mut p, b"*", OUTPUT), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 8);
    assert_eq!(call(&mut p, 0x7000_021c, &[0x7300_0004]), 0);
    assert_eq!(p.last_error().unwrap(), 6);
    assert_eq!(call(&mut p, CLOSE, &[0x7300_0004]), 1);
    assert_eq!(first(&mut p, b"*", OUTPUT), 0x7300_0104);
    for handle in [0, u32::MAX, 0x7200_0004, 0x7300_0004] {
        assert_eq!(call(&mut p, NEXT, &[handle, 0]), 0);
        assert_eq!(p.last_error().unwrap(), 6);
        assert_eq!(call(&mut p, CLOSE, &[handle]), 0);
    }
}

#[test]
fn catalog_validation_rejects_collisions_ancestors_and_bounded_input_overflows() {
    let check = |files: &[FileMetadata<'_>], directories: &[&[u8]]| {
        Process32::load_with_options(
            &find_files_executable::pe32(),
            64,
            ProcessOptions {
                files,
                directories,
                ..ProcessOptions::default()
            },
        )
    };
    for (paths, directories) in [
        (vec![&b"C:\\A"[..], b"c:\\a"], vec![]),
        (vec![b"C:\\A", b"C:\\a\\B"], vec![]),
        (vec![b"C:\\A"], vec![&b"c:\\A"[..]]),
        (vec![b"C:\\A"], vec![b"c:\\a\\B"]),
        (vec![b"relative"], vec![]),
        (vec![b"C:\\CON"], vec![]),
        (vec![b"C:\\trail."], vec![]),
        (vec![b"C:\\"], vec![]),
    ] {
        let files: Vec<_> = paths
            .iter()
            .map(|path| FileMetadata { path, size: 0 })
            .collect();
        assert!(matches!(
            check(&files, &directories),
            Err(LoadError::InvalidProcessParameters)
        ));
    }
    let paths: Vec<_> = (0..4097)
        .map(|i| format!("C:\\file{i}").into_bytes())
        .collect();
    let files: Vec<_> = paths
        .iter()
        .map(|path| FileMetadata { path, size: 0 })
        .collect();
    assert!(check(&files[..4096], &[]).is_ok());
    assert!(matches!(
        check(&files, &[]),
        Err(LoadError::InvalidProcessParameters)
    ));
    let mut paths = Vec::new();
    for index in 0..32 {
        let mut path = b"C:\\".to_vec();
        while path.len() + 256 < 32764 {
            path.extend_from_slice(&[b'a'; 255]);
            path.push(b'\\');
        }
        path.resize(32765, b'b');
        path.extend_from_slice(format!("{index:02}").as_bytes());
        paths.push(path);
    }
    let mut files: Vec<_> = paths
        .iter()
        .map(|path| FileMetadata { path, size: 0 })
        .collect();
    assert!(check(&files, &[]).is_ok());
    files.push(FileMetadata {
        path: b"C:\\extra",
        size: 0,
    });
    assert!(matches!(
        check(&files, &[]),
        Err(LoadError::InvalidProcessParameters)
    ));
    let mut long = b"C:\\".to_vec();
    long.extend_from_slice(&[b'a'; 260]);
    assert!(matches!(
        check(
            &[FileMetadata {
                path: &long,
                size: 0
            }],
            &[]
        ),
        Err(LoadError::InvalidProcessParameters)
    ));
    let mut p = load(&[], &[b"C:\\A", &long]);
    p.memory.write(u64::from(SOURCE), b"*\0").unwrap();
    let before = prepare(&mut p, FIRST, &[SOURCE, OUTPUT]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: FIRST }
    );
    assert_eq!(p.cpu, before);
    assert_eq!(first(&mut p, b"A", OUTPUT), 0x7300_0004);
    assert!(
        check(
            &[FileMetadata {
                path: b"C:\\A",
                size: 0
            }],
            &[b"C:\\AB"]
        )
        .is_ok()
    );
}

#[test]
fn unterminated_input_read_only_output_and_invalid_frames_are_retryable() {
    let mut p = load(&[], &[b"C:\\Data"]);
    p.memory
        .map_zeroed(0x3000_0000, PAGE_SIZE * 8, Permissions::READ_WRITE)
        .unwrap();
    p.memory.write(0x3000_0000, &vec![b'x'; 32768]).unwrap();
    let before = prepare(&mut p, FIRST, &[0x3000_0000, OUTPUT]);
    assert_eq!(
        p.run(1).reason,
        ProcessStop::UnsupportedApi { address: FIRST }
    );
    assert_eq!(p.cpu, before);
    p.memory.write(u64::from(SOURCE), b"*\0").unwrap();
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    let before = prepare(&mut p, FIRST, &[SOURCE, OUTPUT]);
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    p.memory
        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, FIRST, &[SOURCE, OUTPUT]);
    p.cpu.set_register(Register32::Esp, 0x1000_fffc);
    let before = p.cpu;
    assert!(matches!(
        p.run(1).reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!(p.cpu, before);
    assert_eq!(first(&mut p, b"*", OUTPUT), 0x7300_0004);
}

#[test]
fn file_only_parent_directories_exist_without_making_the_file_a_directory() {
    let mut p = load(
        &[FileMetadata {
            path: b"D:\\Base\\Inner\\content",
            size: 0,
        }],
        &[],
    );
    p.memory.write(u64::from(SOURCE), b"d:\\base\0").unwrap();
    assert_eq!(call(&mut p, 0x7000_0230, &[SOURCE]), 1);
    let handle = first(&mut p, b"*", OUTPUT);
    record(&p, OUTPUT, b"Inner", 0, true);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    p.memory
        .write(u64::from(SOURCE), b"Inner\\content\0")
        .unwrap();
    assert_eq!(call(&mut p, 0x7000_0230, &[SOURCE]), 0);
    assert_eq!(p.last_error().unwrap(), 3);
    let handle = first(&mut p, b"Inner\\content", OUTPUT);
    record(&p, OUTPUT, b"content", 0, false);
    assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    assert_eq!(first(&mut p, b"Inner\\content\\*", 0), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 3);
}

#[test]
fn reserved_looking_extensions_are_not_device_names() {
    let mut p = load(
        &[
            FileMetadata {
                path: b"C:\\alpha.NUL",
                size: 1,
            },
            FileMetadata {
                path: b"C:\\beta.COM1",
                size: 2,
            },
        ],
        &[],
    );
    for (pattern, name, size) in [
        (&b"*.nul"[..], &b"alpha.NUL"[..], 1),
        (b"*.com1", b"beta.COM1", 2),
    ] {
        let handle = first(&mut p, pattern, OUTPUT);
        assert_ne!(handle, u32::MAX);
        record(&p, OUTPUT, name, size, false);
        assert_eq!(call(&mut p, CLOSE, &[handle]), 1);
    }
}
