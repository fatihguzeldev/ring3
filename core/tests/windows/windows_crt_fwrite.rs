use super::{file_handle_cases, imported_executable};

use ring3_core::execution::{
    Permissions, Process32, ProcessOptions, ProcessStop, Register32, StopReason,
};

const FOPEN: u32 = 0x7000_0194;
const FWRITE: u32 = 0x7000_05f4;
const FREAD: u32 = 0x7000_0198;
const FCLOSE: u32 = 0x7000_019c;
const STAT: u32 = 0x7000_015c;
const FSEEK: u32 = 0x7000_01a0;
const FTELL: u32 = 0x7000_01a4;
const FIND_FIRST: u32 = 0x7000_0234;
const PATH: u32 = 0x0040_2200;
const MODE: u32 = 0x0040_2300;
const RECORD: u32 = 0x0040_2600;
const STATS: u32 = 0x0040_2800;
const BUFFER: u32 = 0x2300_0000;
const COUNT: usize = 287_527;

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn load() -> Process32 {
    let exe = imported_executable::pe32(&[0xcc], "MSVCRT.dll", &["fopen", "fwrite", "fread"]);
    let mut p = Process32::load_with_options(
        &exe,
        256,
        ProcessOptions {
            directories: &[b"C:\\savegames"],
            ..ProcessOptions::default()
        },
    )
    .unwrap();
    assert_eq!(word(&p, 0x0040_2060), FOPEN);
    assert_eq!(word(&p, 0x0040_2064), FWRITE);
    assert_eq!(word(&p, 0x0040_2068), FREAD);
    p.memory
        .write(u64::from(PATH), b"C:\\\\savegames\\savegame000.mps\0")
        .unwrap();
    p.memory.write(u64::from(MODE), b"wb\0").unwrap();
    p.memory
        .map_zeroed(
            u64::from(BUFFER),
            (COUNT.div_ceil(4096) * 4096) as u64,
            Permissions::READ_WRITE,
        )
        .unwrap();
    p
}

fn crt_call(p: &mut Process32, api: u32, args: &[u32]) -> u32 {
    let mut expected = file_handle_cases::prepare(p, api, args);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    let value = p.cpu.register(Register32::Eax);
    expected.eip = 0x0040_1000;
    expected.set_register(Register32::Esp, file_handle_cases::STACK + 4);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
    value
}

#[test]
fn imported_fwrite_copies_the_tutorial_size_and_updates_position_and_catalog_size() {
    let mut p = load();
    let stream = crt_call(&mut p, FOPEN, &[PATH, MODE]);
    assert_ne!(stream, 0);
    let count = u32::try_from(COUNT).unwrap();
    let bytes: Vec<u8> = (0..COUNT)
        .map(|index| u8::try_from(index % 256).unwrap())
        .collect();
    p.memory.write(u64::from(BUFFER), &bytes).unwrap();
    assert_eq!(crt_call(&mut p, FWRITE, &[BUFFER, 1, count, stream]), count);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), count);
    assert_eq!(crt_call(&mut p, FSEEK, &[stream, 0, 2]), 0);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), count);
    assert_eq!(
        file_handle_cases::call(&mut p, FIND_FIRST, &[PATH, RECORD]),
        0x7300_0004
    );
    assert_eq!(word(&p, RECORD + 32), count);

    assert_eq!(crt_call(&mut p, FSEEK, &[stream, 0, 0]), 0);
    p.memory.write(u64::from(BUFFER), b"ZZZZZZ").unwrap();
    assert_eq!(crt_call(&mut p, FWRITE, &[BUFFER, 3, 2, stream]), 2);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), 6);
    assert_eq!(crt_call(&mut p, FSEEK, &[stream, 0, 2]), 0);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), count);
    assert_eq!(crt_call(&mut p, FCLOSE, &[stream]), 0);
    assert_eq!(crt_call(&mut p, STAT, &[PATH, STATS]), 0);
    assert_eq!(word(&p, STATS + 20), count);
    p.memory.write(u64::from(MODE), b"rb\0").unwrap();
    let read_stream = crt_call(&mut p, FOPEN, &[PATH, MODE]);
    assert_ne!(read_stream, 0);
    p.memory
        .write(u64::from(BUFFER), &vec![0xa5; COUNT])
        .unwrap();
    assert_eq!(
        crt_call(&mut p, FREAD, &[BUFFER, 1, count, read_stream]),
        count
    );
    let mut readback = vec![0; COUNT];
    p.memory.read(u64::from(BUFFER), &mut readback).unwrap();
    let mut expected = bytes;
    expected[..6].copy_from_slice(b"ZZZZZZ");
    assert_eq!(readback, expected);
    assert_eq!(crt_call(&mut p, FREAD, &[BUFFER, 1, 1, read_stream]), 0);
    assert_eq!(crt_call(&mut p, FTELL, &[read_stream]), count);
    assert_eq!(crt_call(&mut p, FCLOSE, &[read_stream]), 0);
}

#[test]
fn forward_seek_zero_fills_and_reopen_truncates_existing_virtual_file() {
    let mut p = load();
    let stream = crt_call(&mut p, FOPEN, &[PATH, MODE]);
    p.memory.write(u64::from(BUFFER), b"abc").unwrap();
    assert_eq!(crt_call(&mut p, FSEEK, &[stream, 10, 0]), 0);
    assert_eq!(crt_call(&mut p, FWRITE, &[BUFFER, 1, 3, stream]), 3);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), 13);
    assert_eq!(
        file_handle_cases::call(&mut p, FIND_FIRST, &[PATH, RECORD]),
        0x7300_0004
    );
    assert_eq!(word(&p, RECORD + 32), 13);
    let next = crt_call(&mut p, FOPEN, &[PATH, MODE]);
    assert_ne!(next, stream);
    assert_eq!(crt_call(&mut p, FSEEK, &[next, 0, 2]), 0);
    assert_eq!(crt_call(&mut p, FTELL, &[next]), 0);
}

#[test]
fn bad_buffer_and_oversize_write_leave_stream_and_file_unchanged() {
    let mut p = load();
    let stream = crt_call(&mut p, FOPEN, &[PATH, MODE]);
    assert_eq!(crt_call(&mut p, FWRITE, &[0, 0, 1, stream]), 0);
    let before = file_handle_cases::prepare(&mut p, FWRITE, &[BUFFER, 1, 1, stream + 1]);
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::UnsupportedApi { address: FWRITE }
    );
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    let before = file_handle_cases::prepare(&mut p, FWRITE, &[u32::MAX - 1, 1, 16, stream]);
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), 0);
    assert_eq!(
        crt_call(&mut p, FWRITE, &[BUFFER, 1, 64 * 1024 * 1024 + 1, stream]),
        0
    );
    assert_eq!(file_handle_cases::word(&p, file_handle_cases::ERRNO), 28);
    assert_eq!(crt_call(&mut p, FTELL, &[stream]), 0);
}
