use super::super::BitmapLayout32;

#[test]
fn rgb555_truncation_and_returned_colors_match_public_windows_reference() {
    // authored Windows i386 oracle; retained gdi-casing.json and search-message-partial-reference.json.
    let layout = BitmapLayout32::new(1, -1, 16, true).unwrap();
    for (color, raw, returned) in [
        (0x010101, 0x0000, 0x000000),
        (0x040404, 0x0000, 0x000000),
        (0x070707, 0x0000, 0x000000),
        (0x080808, 0x0421, 0x080808),
        (0x7f7f7f, 0x3def, 0x7b7b7b),
        (0xffffff, 0x7fff, 0xffffff),
        (0x202020, 0x1084, 0x212121),
        (0x404040, 0x2108, 0x424242),
        (0xc0c0c0, 0x6318, 0xc6c6c6),
    ] {
        let mut bytes = [0xa5; 4];
        layout.write_pixel(&mut bytes, &[], 0, 0, color);
        assert_eq!(
            u16::from_le_bytes(bytes[..2].try_into().unwrap()),
            raw,
            "color {color:06x}"
        );
        assert_eq!(
            layout.read_pixel(&bytes, &[], 0, 0),
            returned,
            "color {color:06x}"
        );
        assert_eq!(&bytes[2..], &[0xa5; 2]);
    }
}
