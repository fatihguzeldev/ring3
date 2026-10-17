use super::*;
use crate::memory::{PageRange, Permissions};

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn captured_ansi_file_filter_and_custom_pair_use_configured_dbcs_and_exact_custom_capacity() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let cp = CodePage32::from_id(932).unwrap();
    let file = cp
        .encode(&"日本.txt\0".encode_utf16().collect::<Vec<_>>(), false)
        .unwrap()
        .0;
    let filter = cp
        .encode(
            &"画像\0*.bmp;*.ico\0All\0*.*\0\0"
                .encode_utf16()
                .collect::<Vec<_>>(),
            false,
        )
        .unwrap()
        .0;
    let custom = cp
        .encode(&"選択\0*.txt\0".encode_utf16().collect::<Vec<_>>(), false)
        .unwrap()
        .0;
    let mut header = [0; 88];
    for (at, value) in [
        (0, 88),
        (12, 0x10200),
        (16, 0x10300),
        (20, custom.len() as u32),
        (28, 0x10100),
        (32, 128),
    ] {
        word(&mut header, at, value);
    }
    for (address, bytes) in [
        (0x10000, header.as_slice()),
        (0x10100, file.as_slice()),
        (0x10200, filter.as_slice()),
        (0x10300, custom.as_slice()),
    ] {
        memory.write(GuestAddress(address), bytes).unwrap();
    }
    let input = CapturedFileDialog32::read(&mut memory, 0x10000, false, cp, 4096).unwrap();
    assert_eq!(
        input.initial_file,
        "日本.txt".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(input.filters.entries.len(), 2);
    assert!(input.filters.entries[0].matches(&"game.ICO".encode_utf16().collect::<Vec<_>>()));
    assert_eq!(
        input.filters.custom.as_ref().unwrap().label,
        "選択".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        input.filters.custom.as_ref().unwrap().pattern,
        "*.txt".encode_utf16().collect::<Vec<_>>()
    );
    memory.write(GuestAddress(0x10100), b"erased\0").unwrap();
    assert_eq!(
        input.initial_file,
        "日本.txt".encode_utf16().collect::<Vec<_>>()
    );
    input.outputs.check(&memory).unwrap();
}
