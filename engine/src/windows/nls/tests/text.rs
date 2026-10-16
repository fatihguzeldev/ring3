use crate::windows::{
    nls::{NlsError32, linguistics::*},
    text::{CodePage32, ConversionError32},
};
#[test]
fn unicode_transcoding_preserves_supplementary_scalars_and_explicit_substitution() {
    let units = CodePage32::Utf8
        .decode_windows("A😀é".as_bytes(), 8)
        .unwrap();
    assert_eq!(
        CodePage32::Utf8
            .encode_windows(&units, 0x80, b'?')
            .unwrap()
            .0,
        "A😀é".as_bytes()
    );
    assert_eq!(
        CodePage32::Utf8.decode_windows(&[0xff], 8),
        Err(ConversionError32::NoTranslation)
    );
    assert_eq!(
        CodePage32::Windows1252
            .encode_windows(&[0x100, 0x221e], 0, b'!')
            .unwrap(),
        (vec![b'A', b'8'], false)
    );
    assert_eq!(
        CodePage32::Windows1252
            .encode_windows(&[0x100, 0x221e], 0x400, b'!')
            .unwrap(),
        (vec![b'!', b'!'], true)
    );
    assert_eq!(
        CodePage32::Windows1252.decode_windows(&[0xe9], 2).unwrap(),
        [b'e' as u16, 0x301]
    );
}
#[test]
fn linguistic_keys_and_comparisons_share_unicode_collation_and_character_data() {
    let text = |value: &str| value.encode_utf16().collect::<Vec<_>>();
    assert_eq!(
        compare(&text("é"), &text("e\u{301}"), 0).unwrap(),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        compare(&text("É"), &text("e"), 3).unwrap(),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        compare(&text("item2"), &text("item10"), 8).unwrap(),
        std::cmp::Ordering::Less
    );
    for flags in [0, 1, 3, 8] {
        let left = text("Résumé2");
        let right = text("resume10");
        assert_eq!(
            sort_key(&left, flags)
                .unwrap()
                .cmp(&sort_key(&right, flags).unwrap()),
            compare(&left, &right, flags).unwrap()
        );
    }
    let bits = character_types(&[0xc9, 0x3b1, 0x4e2d, 0x661, 0x20], 1).unwrap();
    assert_eq!(bits[0] & 0x101, 0x101);
    assert_ne!(bits[1] & 0x100, 0);
    assert_ne!(bits[2] & 0x100, 0);
    assert_ne!(bits[3] & 4, 0);
    assert_ne!(bits[4] & 0x48, 0);
    assert_eq!(
        character_types(&[0x5d0, 0x627, 0x661], 2).unwrap(),
        [2, 2, 6]
    );
    assert_eq!(map_string(&text("straße"), 0x200).unwrap(), text("STRASSE"));
    assert_eq!(character_types(&[65], 0), Err(NlsError32::InvalidFlags));
}

#[test]
fn kana_width_mapping_handles_voiced_pairs_and_keeps_other_compatibility_characters() {
    let units = |value: &str| value.encode_utf16().collect::<Vec<_>>();
    assert_eq!(
        map_string(&units("Ａ ガ 。"), 0x400000).unwrap(),
        units("A ｶﾞ ｡")
    );
    assert_eq!(
        map_string(&units("A ｶﾞ ｡"), 0x800000).unwrap(),
        units("Ａ　ガ　。")
    );
    assert_eq!(
        compare(&units("ｶﾞ"), &units("ガ"), 0x20000).unwrap(),
        std::cmp::Ordering::Equal
    );
    assert_ne!(
        compare(&units("①"), &units("1"), 0x20000).unwrap(),
        std::cmp::Ordering::Equal
    );
}

#[test]
fn locale_specific_case_and_collation_cover_turkish_and_swedish() {
    use crate::windows::nls::linguistics::{compare_for_locale, map_string_for_locale};
    use std::cmp::Ordering;
    let units = |text: &str| text.encode_utf16().collect::<Vec<_>>();
    assert_eq!(
        map_string_for_locale(&units("iıIİ"), 0x01000200, 0x41f).unwrap(),
        units("İIIİ")
    );
    assert_eq!(
        map_string_for_locale(&units("ΟΣ"), 0x100, 0x408).unwrap(),
        units("ος")
    );
    assert_eq!(
        compare_for_locale(&units("I"), &units("ı"), 1, 0x41f).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale(&units("I"), &units("ı"), 0x08000001, 0x41f).unwrap(),
        Ordering::Equal
    );
    assert_eq!(
        compare_for_locale(&units("z"), &units("ä"), 0, 0x41d).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        compare_for_locale(&units("z"), &units("ä"), 0, 0x407).unwrap(),
        Ordering::Greater
    );
}
