use super::short_path_executable;

use ring3_core::execution::{
    FileMetadata, Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_0294;
const SOURCE: u32 = 0x0040_2400;
const OUTPUT: u32 = 0x0040_2800;
const STACK: u32 = 0x1000_ff00;

#[test]
fn imported_short_path_uses_the_existing_no_alias_directory_profile() {
    for budget in [1, 50] {
        let mut p = Process32::load(&short_path_executable::pe32(), 32).unwrap();
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
        assert_eq!(counts, (10, 2));
        assert_eq!(p.cpu.register(Register32::Eax), 4);
        assert_eq!(p.cpu.register(Register32::Ebx), 3);
        assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
        let mut bytes = [0; 4];
        p.memory.read(0x0040_21a0, &mut bytes).unwrap();
        assert_eq!(&bytes, b"c:\\\0");
    }
}

fn load() -> Process32 {
    Process32::load_with_options(
        &short_path_executable::pe32(),
        64,
        ProcessOptions {
            files: &[FileMetadata {
                path: b"C:\\Long Folder\\Long File.bin",
                size: 42,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap()
}

fn read(p: &Process32, address: u32, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
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

fn call(p: &mut Process32, api: u32, args: &[u32], expected: u32) {
    prepare(p, api, args);
    let mut cpu = p.cpu;
    let pages = p.memory.mapped_pages();
    let run = p.run(1);
    assert_eq!(
        run.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((run.instructions, run.api_calls), (0, 1));
    cpu.eip = 0x0040_1000;
    cpu.set_register(Register32::Eax, expected);
    cpu.set_register(
        Register32::Esp,
        STACK
            + if api == 0x7000_0188 {
                4
            } else {
                u32::try_from(args.len() + 1).unwrap() * 4
            },
    );
    assert_eq!(p.cpu, cpu);
    assert_eq!(p.memory.mapped_pages(), pages);
}

fn stopped(p: &mut Process32, unsupported: bool) {
    let cpu = p.cpu;
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
}

#[test]
fn no_alias_paths_preserve_spelling_and_follow_directory_and_removal_state() {
    let mut p = load();
    for path in [
        b"C:\\Long Folder\\Long File.bin\0".as_slice(),
        b"long folder\\LONG FILE.BIN\0",
        b"C:\\\\Long Folder\\.\\Long File.bin\0",
        b"Long Folder\\\0",
    ] {
        p.memory.write(u64::from(SOURCE), path).unwrap();
        call(
            &mut p,
            API,
            &[SOURCE, OUTPUT, 260],
            u32::try_from(path.len() - 1).unwrap(),
        );
        assert_eq!(read(&p, OUTPUT, path.len()), path);
    }
    p.memory.write(u64::from(SOURCE), b"Long Folder\0").unwrap();
    call(&mut p, 0x7000_0230, &[SOURCE], 1);
    p.memory
        .write(u64::from(SOURCE), b"Long File.bin\0")
        .unwrap();
    call(&mut p, API, &[SOURCE, OUTPUT, 260], 13);
    call(&mut p, 0x7000_0188, &[SOURCE], 0);
    call(&mut p, API, &[SOURCE, OUTPUT, 260], 0);
    assert_eq!(p.last_error().unwrap(), 2);
    p.memory.write(u64::from(SOURCE), b".\0").unwrap();
    call(&mut p, API, &[SOURCE, OUTPUT, 260], 1);
}

#[test]
fn sizing_and_overlapping_output_use_a_snapshot_without_partial_copies() {
    let mut p = load();
    for capacity in [0, 1, 3] {
        p.memory.write(u64::from(SOURCE), b"C:\\\0").unwrap();
        call(&mut p, API, &[SOURCE, u32::MAX, capacity], 4);
    }
    for output in [SOURCE, SOURCE + 1, SOURCE - 1] {
        p.memory.write(u64::from(SOURCE - 2), &[0x55; 16]).unwrap();
        p.memory.write(u64::from(SOURCE), b"C:\\\0").unwrap();
        call(&mut p, API, &[SOURCE, output, 4], 3);
        assert_eq!(read(&p, output, 4), b"C:\\\0");
        assert_eq!(read(&p, SOURCE + 8, 1), [0x55]);
    }
}

#[test]
fn missing_and_empty_paths_report_errors_without_touching_output() {
    let mut p = load();
    p.memory.write(u64::from(OUTPUT), &[0x55; 32]).unwrap();
    for (path, error) in [
        (b"missing\0".as_slice(), 2),
        (b"missing\\child\0", 3),
        (b"\0", 161),
        (b"bad*name\0", 123),
    ] {
        p.memory.write(u64::from(SOURCE), path).unwrap();
        call(&mut p, API, &[SOURCE, OUTPUT, 260], 0);
        assert_eq!(p.last_error().unwrap(), error);
        assert_eq!(read(&p, OUTPUT, 32), [0x55; 32]);
    }
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::READ)
        .unwrap();
    prepare(&mut p, API, &[SOURCE, OUTPUT, 260]);
    stopped(&mut p, false);
    p.memory
        .protect(0x7ffd_e000, 4096, Permissions::NONE)
        .unwrap();
    p.memory
        .protect(0x7000_2000, 4096, Permissions::NONE)
        .unwrap();
    p.memory.write(u64::from(SOURCE), b"C:\\\0").unwrap();
    call(&mut p, API, &[SOURCE, OUTPUT, 260], 3);
    call(&mut p, API, &[SOURCE, 0, 0], 4);
}

#[test]
fn output_input_frame_and_budget_faults_preserve_state() {
    let mut p = load();
    p.memory.write(u64::from(SOURCE), b"C:\\\0").unwrap();
    p.memory.write(0x0040_2ffe, &[0x55; 2]).unwrap();
    prepare(&mut p, API, &[SOURCE, 0x0040_2ffe, 4]);
    stopped(&mut p, false);
    assert_eq!(read(&p, 0x0040_2ffe, 2), [0x55; 2]);
    p.memory
        .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
        .unwrap();
    prepare(&mut p, API, &[SOURCE, u32::MAX - 1, 4]);
    stopped(&mut p, false);
    p.memory.write(u64::from(u32::MAX), b"x").unwrap();
    for source in [0, 0x6000_0000, u32::MAX] {
        prepare(&mut p, API, &[source, OUTPUT, 260]);
        stopped(&mut p, false);
    }
    prepare(&mut p, API, &[SOURCE, OUTPUT, 260]);
    let cpu = p.cpu;
    assert_eq!(p.run(0).api_calls, 0);
    assert_eq!(p.cpu, cpu);
    p.cpu.set_register(Register32::Esp, 0x1000_fff4);
    stopped(&mut p, false);
}

#[test]
fn bounded_paths_and_unsupported_names_reuse_attribute_admission() {
    let mut path = b"C:\\".to_vec();
    path.extend_from_slice(&[b'a'; 250]);
    path.extend_from_slice(b"\\bbbbb");
    assert_eq!(path.len(), 259);
    let mut p = Process32::load_with_options(
        &short_path_executable::pe32(),
        64,
        ProcessOptions {
            files: &[FileMetadata {
                path: &path,
                size: 0,
            }],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    path.push(0);
    p.memory.write(u64::from(SOURCE), &path).unwrap();
    call(&mut p, API, &[SOURCE, OUTPUT, 260], 259);
    assert_eq!(read(&p, OUTPUT, 260), path);
    path.insert(259, b'b');
    for value in [path.as_slice(), b"\\\\server\\share\0", b"NUL\0", b"\x80\0"] {
        p.memory.write(u64::from(SOURCE), value).unwrap();
        prepare(&mut p, API, &[SOURCE, OUTPUT, 260]);
        stopped(&mut p, true);
    }
}
