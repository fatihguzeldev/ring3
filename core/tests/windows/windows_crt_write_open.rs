use super::file_handle_cases::{ERRNO, PATH, call, process, word};

use ring3_core::execution::{ProcessStop, Register32, StopReason};

const CREATE: u32 = 0x7000_05f0;
const FOPEN: u32 = 0x7000_0194;
const FCLOSE: u32 = 0x7000_019c;
const ATTRIBUTES: u32 = 0x7000_0290;
const FIND_FIRST: u32 = 0x7000_0234;
const FIND_NEXT: u32 = 0x7000_0238;
const FIND_CLOSE: u32 = 0x7000_023c;
const MODE: u32 = 0x0040_2300;
const PATTERN: u32 = 0x0040_2400;
const RECORD: u32 = 0x0040_2600;

fn name(p: &ring3_core::execution::Process32) -> Vec<u8> {
    let mut bytes = [0; 260];
    p.memory.read(u64::from(RECORD + 44), &mut bytes).unwrap();
    bytes.into_iter().take_while(|&byte| byte != 0).collect()
}

#[test]
fn write_binary_open_creates_a_visible_guest_file_without_shifting_open_search_entries() {
    let mut p = process(0);
    p.memory
        .write(u64::from(PATH), b"C:\\root\\savegames\0")
        .unwrap();
    assert_eq!(call(&mut p, CREATE, &[PATH, 0]), 1);
    p.memory
        .write(u64::from(PATTERN), b"C:\\root\\*\0")
        .unwrap();
    assert_eq!(call(&mut p, FIND_FIRST, &[PATTERN, RECORD]), 0x7300_0004);
    assert_eq!(name(&p), b"empty.bin");

    p.memory
        .write(u64::from(PATH), b"C:\\\\root\\savegames\\savegame000.mps\0")
        .unwrap();
    p.memory.write(u64::from(MODE), b"wb\0").unwrap();
    let stream = call(&mut p, FOPEN, &[PATH, MODE]);
    assert_ne!(stream, 0);
    let second_stream = call(&mut p, FOPEN, &[PATH, MODE]);
    assert_ne!(second_stream, 0);
    assert_ne!(second_stream, stream);
    assert_eq!(word(&p, ERRNO), 88);
    assert_eq!(call(&mut p, ATTRIBUTES, &[PATH]), 128);
    assert_eq!(call(&mut p, FIND_NEXT, &[0x7300_0004, RECORD]), 1);
    assert_eq!(name(&p), b"metadata.bin");
    assert_eq!(call(&mut p, FIND_NEXT, &[0x7300_0004, RECORD]), 1);
    assert_eq!(name(&p), b"sample.bin");
    assert_eq!(call(&mut p, FIND_NEXT, &[0x7300_0004, RECORD]), 1);
    assert_eq!(name(&p), b"savegames");
    assert_eq!(call(&mut p, FIND_CLOSE, &[0x7300_0004]), 1);
    p.memory
        .write(u64::from(PATTERN), b"C:\\root\\savegames\\*\0")
        .unwrap();
    assert_eq!(call(&mut p, FIND_FIRST, &[PATTERN, RECORD]), 0x7300_0008);
    assert_eq!(name(&p), b"savegame000.mps");
    assert_eq!(call(&mut p, FIND_NEXT, &[0x7300_0008, RECORD]), 0);
    assert_eq!(p.last_error().unwrap(), 18);
    assert_eq!(call(&mut p, FCLOSE, &[stream]), 0);
    assert_eq!(call(&mut p, FCLOSE, &[second_stream]), 0);
}

#[test]
fn write_open_refuses_missing_parent_and_directory_without_allocating_a_stream() {
    let mut p = process(0);
    p.memory.write(u64::from(MODE), b"wb\0").unwrap();
    p.memory
        .write(u64::from(PATH), b"C:\\root\\missing\\new.mps\0")
        .unwrap();
    assert_eq!(call(&mut p, FOPEN, &[PATH, MODE]), 0);
    assert_eq!(word(&p, ERRNO), 2);
    p.memory.write(u64::from(PATH), b"C:\\root\0").unwrap();
    assert_eq!(call(&mut p, FOPEN, &[PATH, MODE]), 0);
    assert_eq!(word(&p, ERRNO), 13);
    p.memory
        .write(u64::from(PATH), b"C:\\root\\new.mps\0")
        .unwrap();
    assert_ne!(call(&mut p, FOPEN, &[PATH, MODE]), 0);
}

#[test]
fn invalid_path_read_preserves_cpu_and_catalog() {
    let mut p = process(0);
    p.memory.write(u64::from(MODE), b"wb\0").unwrap();
    let before = super::file_handle_cases::prepare(&mut p, FOPEN, &[u32::MAX - 1, MODE]);
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.memory
        .write(u64::from(PATH), b"C:\\root\\new.mps\0")
        .unwrap();
    assert_eq!(call(&mut p, ATTRIBUTES, &[PATH]), u32::MAX);
    assert_eq!(p.last_error().unwrap(), 2);
    assert_eq!(p.cpu.register(Register32::Eax), u32::MAX);
}
