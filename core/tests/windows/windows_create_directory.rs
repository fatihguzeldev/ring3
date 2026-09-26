use super::imported_executable;

use ring3_core::execution::{
    FileMetadata, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const CREATE: u32 = 0x7000_05f0;
const ATTRIBUTES: u32 = 0x7000_0290;
const CHANGE: u32 = 0x7000_0230;
const FIND_FIRST: u32 = 0x7000_0234;
const FIND_NEXT: u32 = 0x7000_0238;
const FIND_CLOSE: u32 = 0x7000_023c;
const IAT: u32 = 0x0040_2060;
const STACK: u32 = 0x1000_ff00;
const PATH: u32 = 0x0040_2400;
const PATTERN: u32 = 0x0040_2480;
const RECORD: u32 = 0x0040_2600;
const RETURN: u32 = 0x0040_1000;

fn read(process: &Process32, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

fn write(process: &mut Process32, address: u32, bytes: &[u8]) {
    process.memory.write(u64::from(address), bytes).unwrap();
}

fn load() -> Process32 {
    let bytes = imported_executable::pe32(&[0xcc], "KERNEL32.dll", &["CreateDirectoryA"]);
    let files = [FileMetadata {
        path: b"C:\\z.bin",
        size: 42,
    }];
    let process = Process32::load_with_options(
        &bytes,
        64,
        ProcessOptions {
            directories: &[b"C:\\data"],
            files: &files,
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        u32::from_le_bytes(read(&process, IAT, 4).try_into().unwrap()),
        CREATE
    );
    process
}

fn prepare(process: &mut Process32, api: u32, args: &[u32]) {
    process.cpu.eip = api;
    process.cpu.set_register(Register32::Esp, STACK);
    process.cpu.set_register(Register32::Eax, 0x5555_5555);
    for (index, value) in std::iter::once(RETURN)
        .chain(args.iter().copied())
        .enumerate()
    {
        write(
            process,
            STACK + u32::try_from(index).unwrap() * 4,
            &value.to_le_bytes(),
        );
    }
}

fn call(process: &mut Process32, api: u32, args: &[u32], expected: u32) {
    prepare(process, api, args);
    let mut cpu = process.cpu;
    let result = process.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    cpu.eip = RETURN;
    cpu.set_register(
        Register32::Esp,
        STACK + u32::try_from(args.len() + 1).unwrap() * 4,
    );
    cpu.set_register(Register32::Eax, expected);
    assert_eq!(process.cpu, cpu);
}

fn path(process: &mut Process32, bytes: &[u8]) {
    write(process, PATH, bytes);
}

fn name(process: &Process32) -> Vec<u8> {
    read(process, RECORD + 44, 260)
        .into_iter()
        .take_while(|&byte| byte != 0)
        .collect()
}

#[test]
fn created_directory_is_visible_to_attributes_change_and_search_without_moving_open_sources() {
    let mut process = load();
    write(&mut process, 0x7ffd_e034, &77_u32.to_le_bytes());
    write(&mut process, PATTERN, b"C:\\*\0");
    call(&mut process, FIND_FIRST, &[PATTERN, RECORD], 0x7300_0004);
    assert_eq!(name(&process), b"data");

    path(&mut process, b"C:\\\\savegames\0");
    call(&mut process, CREATE, &[PATH, 0], 1);
    assert_eq!(process.last_error().unwrap(), 77);
    call(&mut process, ATTRIBUTES, &[PATH], 16);
    call(&mut process, CHANGE, &[PATH], 1);

    call(&mut process, FIND_NEXT, &[0x7300_0004, RECORD], 1);
    assert_eq!(name(&process), b"z.bin");
    assert_eq!(read(&process, RECORD, 4), 128_u32.to_le_bytes());
    call(&mut process, FIND_CLOSE, &[0x7300_0004], 1);

    write(&mut process, PATTERN, b"C:\\savegames\0");
    call(&mut process, FIND_FIRST, &[PATTERN, RECORD], 0x7300_0008);
    assert_eq!(name(&process), b"savegames");
    assert_eq!(read(&process, RECORD, 4), 16_u32.to_le_bytes());
}

#[test]
fn duplicate_and_missing_parent_report_windows_errors_without_creating_extra_paths() {
    let mut process = load();
    path(&mut process, b"C:\\savegames\0");
    call(&mut process, CREATE, &[PATH, 0], 1);
    call(&mut process, CREATE, &[PATH, 0], 0);
    assert_eq!(process.last_error().unwrap(), 183);

    path(&mut process, b"C:\\missing\\child\0");
    call(&mut process, CREATE, &[PATH, 0], 0);
    assert_eq!(process.last_error().unwrap(), 3);
    call(&mut process, ATTRIBUTES, &[PATH], u32::MAX);
    assert_eq!(process.last_error().unwrap(), 3);

    path(&mut process, b"C:\\z.bin\0");
    call(&mut process, CREATE, &[PATH, 0], 0);
    assert_eq!(process.last_error().unwrap(), 183);
}

#[test]
fn invalid_guest_source_and_nonnull_security_attributes_are_atomic() {
    let mut process = load();
    prepare(&mut process, CREATE, &[u32::MAX - 1, 0]);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);

    path(&mut process, b"C:\\savegames\0");
    prepare(&mut process, CREATE, &[PATH, 1]);
    let before = process.cpu;
    let result = process.run(1);
    assert!(matches!(result.reason, ProcessStop::UnsupportedApi { .. }));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(process.cpu, before);
    call(&mut process, ATTRIBUTES, &[PATH], u32::MAX);
}

#[test]
fn created_directory_catalog_has_a_bounded_capacity() {
    let mut process = load();
    for index in 0..256 {
        path(&mut process, format!("C:\\new{index}\0").as_bytes());
        call(&mut process, CREATE, &[PATH, 0], 1);
    }
    path(&mut process, b"C:\\overflow\0");
    call(&mut process, CREATE, &[PATH, 0], 0);
    assert_eq!(process.last_error().unwrap(), 8);
    call(&mut process, ATTRIBUTES, &[PATH], u32::MAX);
    assert_eq!(process.last_error().unwrap(), 2);
}
