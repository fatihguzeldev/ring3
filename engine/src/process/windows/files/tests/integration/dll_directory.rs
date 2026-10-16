use super::*;
use crate::windows::WindowsApi32;

const SET_A: u32 = 0x0001_0021;
const SET_W: u32 = 0x0001_0022;
const GET_A: u32 = 0x0001_0023;
const GET_W: u32 = 0x0001_0024;

fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}

fn get(engine: &mut EngineInstance, wide: bool, expected: &[u8]) {
    write(engine, 0x2000, &[0xcc; 128]);
    let length = complete(engine, if wide { GET_W } else { GET_A }, &[64, 0x2000]);
    assert_eq!(
        length as usize,
        expected.len() / if wide { 2 } else { 1 } - 1
    );
    let mut output = vec![0; expected.len() + 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x2000), &mut output)
        .unwrap();
    assert_eq!(&output[..expected.len()], expected);
    assert_eq!(&output[expected.len()..], &[0xcc; 4]);
}

#[test]
fn captured_dll_directory_uses_the_process_codepage_and_preserves_the_unicode_path() {
    let mut engine = engine();
    write(&mut engine, 0x1000, b"C:\\caf\xe9\0");
    assert_eq!(complete(&mut engine, SET_A, &[0x1000]), 1);
    let wide: Vec<u8> = "C:\\café"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    get(&mut engine, true, &wide);
    get(&mut engine, false, b"C:\\caf\xe9\0");
    super::super::support::path(engine.memory.as_mut().unwrap(), 0x1000, r"C:\plugins");
    assert_eq!(complete(&mut engine, SET_W, &[0x1000]), 1);
    get(&mut engine, false, b"C:\\plugins\0");
}

#[test]
fn captured_dll_directory_rejects_inaccessible_input_without_replacing_the_old_selection() {
    let mut engine = engine();
    super::super::support::path(engine.memory.as_mut().unwrap(), 0x1000, r"C:\plugins");
    assert_eq!(complete(&mut engine, SET_W, &[0x1000]), 1);
    for setter in [SET_A, SET_W] {
        assert_eq!(complete(&mut engine, setter, &[0x900000]), 0);
        assert_eq!(
            complete(&mut engine, WindowsApi32::GetLastError.id(), &[]),
            998
        );
        get(&mut engine, false, b"C:\\plugins\0");
    }
    assert_eq!(complete(&mut engine, SET_W, &[0]), 1);
    get(&mut engine, false, b"\0");
    get(&mut engine, true, &[0, 0]);
}

#[test]
fn captured_dll_directory_short_buffers_match_native_lengths_and_preserve_relative_text() {
    let mut engine = engine();
    for getter in [GET_A, GET_W] {
        assert_eq!(complete(&mut engine, getter, &[0, 0]), 1);
    }
    super::super::support::path(
        engine.memory.as_mut().unwrap(),
        0x1000,
        r"relative\..\child",
    );
    assert_eq!(complete(&mut engine, SET_W, &[0x1000]), 1);
    for (getter, width) in [(GET_A, 1usize), (GET_W, 2)] {
        assert_eq!(complete(&mut engine, getter, &[0, 0]), 18);
        write(&mut engine, 0x2000, &[0xcc; 16]);
        assert_eq!(complete(&mut engine, getter, &[3, 0x2000]), 18);
        let mut output = [0; 16];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x2000), &mut output)
            .unwrap();
        assert_eq!(&output[..width], &vec![0; width]);
        assert_eq!(&output[width..], &vec![0xcc; 16 - width]);
    }
    get(&mut engine, false, b"relative\\..\\child\0");
}
