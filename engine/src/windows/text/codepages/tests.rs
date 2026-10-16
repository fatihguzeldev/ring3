use super::CodePage32;
#[test]
fn multilingual_windows_and_oem_pages_round_trip_literal_bytes() {
    for (id, bytes, text) in [
        (1254, &[0xfd][..], "ı"),
        (1251, &[0xef, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2], "привет"),
        (932, &[0x82, 0xa0], "あ"),
        (936, &[0xd6, 0xd0], "中"),
        (850, &[0x82], "é"),
    ] {
        let cp = CodePage32::from_id(id).unwrap();
        let units: Vec<_> = text.encode_utf16().collect();
        assert_eq!(cp.decode(bytes, true).unwrap(), units);
        assert_eq!(cp.encode(&units, true).unwrap(), (bytes.to_vec(), false));
    }
    let japanese = CodePage32::from_id(932).unwrap();
    assert!(japanese.decode(&[0x82], true).is_err());
    assert_eq!(japanese.decode(&[0x82], false).unwrap(), [0xfffd]);
    assert_eq!(japanese.maximum_char_size(), 2);
    assert!(japanese.is_lead_byte(0x82));
    assert!(!japanese.is_lead_byte(0xa0));
}

#[test]
fn windows_best_fit_and_composite_flags_keep_literal_dbcs_and_default_receipts() {
    let turkish = CodePage32::from_id(1254).unwrap();
    assert_eq!(
        turkish.encode_windows(&[0x100], 0, b'!').unwrap(),
        (vec![b'A'], false)
    );
    assert_eq!(
        turkish.encode_windows(&[0x100], 0x400, b'!').unwrap(),
        (vec![b'!'], true)
    );
    assert_eq!(
        turkish
            .encode_windows(&[b'q' as u16, 0x301], 0x210, b'!')
            .unwrap(),
        (vec![b'q'], false)
    );
    let japanese = CodePage32::from_id(932).unwrap();
    assert_eq!(
        japanese.encode_windows(&[0xa5], 0, b'?').unwrap(),
        (vec![0x5c], false)
    );
    assert_eq!(
        japanese.encode_windows(&[0xa5], 0x400, b'?').unwrap(),
        (vec![b'?'], true)
    );
    let iso_turkish = CodePage32::from_id(28599).unwrap();
    assert_eq!(
        iso_turkish.decode(&[0x80, 0xdd], true).unwrap(),
        [0x80, 0x130]
    );
    assert_eq!(
        turkish.decode(&[0x80, 0xdd], true).unwrap(),
        [0x20ac, 0x130]
    );
}
