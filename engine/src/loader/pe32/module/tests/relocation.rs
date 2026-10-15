use super::{support as pe, *};

fn image(entries: &[u16]) -> Vec<u8> {
    let mut bytes = pe::empty_data();
    pe::directory(&mut bytes, 5, 0x2100, 8 + entries.len() as u32 * 2);
    pe::word(&mut bytes, 0x2100, 0x1000);
    pe::word(&mut bytes, 0x2104, 8 + entries.len() as u32 * 2);
    for (index, entry) in entries.iter().enumerate() {
        pe::put16(&mut bytes, pe::offset(0x2108) + index * 2, *entry);
    }
    bytes
}

#[test]
fn all_ia32_fixups_use_literal_carries_and_signed_highadj_addend() {
    let mut bytes = image(&[0x1004, 0x2006, 0x3008, 0x400c, 0xfffe, 0]);
    pe::put16(&mut bytes, pe::offset(0x1004), 0x0040);
    pe::put16(&mut bytes, pe::offset(0x1006), 0x0022);
    pe::word(&mut bytes, 0x1008, 0x0040_2000);
    pe::put16(&mut bytes, pe::offset(0x100c), 0x0041);
    let parsed = parse_module(&bytes).unwrap();
    for (base, high, pointer, adjusted) in [
        (0x0041_0000, 0x0041u16, 0x0041_2000u32, 0x0042u16),
        (0x003f_0000, 0x003f, 0x003f_2000, 0x0040),
    ] {
        let pages = parsed.prepare_pages(&bytes, base).unwrap();
        assert_eq!(&pages[1].bytes[4..6], &high.to_le_bytes());
        assert_eq!(&pages[1].bytes[6..8], &[0x22, 0]);
        assert_eq!(&pages[1].bytes[8..12], &pointer.to_le_bytes());
        assert_eq!(&pages[1].bytes[12..14], &adjusted.to_le_bytes());
    }
}

#[test]
fn relocation_can_cross_mapped_page_padding_and_touch_zero_filled_storage() {
    let mut bytes = image(&[0x3ffe, 0]);
    pe::data(&mut bytes, 0x2000, &[0xaa, 0xbb]);
    let parsed = parse_module(&bytes).unwrap();
    let pages = parsed.prepare_pages(&bytes, 0x0041_0000).unwrap();
    assert_eq!(&pages[1].bytes[4094..], &[0, 0]);
    assert_eq!(&pages[2].bytes[..2], &[0xab, 0xbb]);
    let mut bytes = image(&[0x3200, 0]);
    pe::word(&mut bytes, 0x2100, 0x2000);
    let parsed = parse_module(&bytes).unwrap();
    let pages = parsed.prepare_pages(&bytes, 0x0041_0000).unwrap();
    assert_eq!(&pages[2].bytes[0x200..0x204], &[0, 0, 1, 0]);
}

#[test]
fn missing_highadj_pair_overlap_and_unmapped_targets_fail_without_publishing_pages() {
    assert_eq!(
        parse_module(&image(&[0x4004])).err(),
        Some(LoadError::Malformed)
    );
    assert_eq!(
        parse_module(&image(&[0x3004, 0x3006])).err(),
        Some(LoadError::Malformed)
    );
    let mut bytes = image(&[0x3000, 0]);
    pe::word(&mut bytes, 0x2100, 0x3000);
    assert_eq!(parse_module(&bytes).err(), Some(LoadError::Malformed));
}
