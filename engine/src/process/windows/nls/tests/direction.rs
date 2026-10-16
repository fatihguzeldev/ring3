use super::*;
use crate::windows::nls::{NlsError32, calendar::SystemTime32};
// native x86 oracle: windows build 20348, run 38929202570, source 3dd5984d323fdf0bf41c363ca3475edd745a025f.

fn date(engine: &mut EngineInstance) {
    write(
        engine,
        0x10000,
        &SystemTime32 {
            year: 2001,
            month: 2,
            day: 3,
            ..Default::default()
        }
        .bytes(),
    );
}

#[test]
fn native_direction_marks_belong_to_date_fields_and_quoted_literal_tokens() {
    let mut engine = engine();
    date(&mut engine);
    for (picture, expected) in [
        ("yyyy/MM/dd", "\u{200e}2001/\u{200e}02/\u{200e}03"),
        (
            "'prefix' d-M-yyyy 'suffix'",
            "\u{200e}prefix \u{200e}3-\u{200e}2-\u{200e}2001 \u{200e}suffix",
        ),
        (
            "yyyy'-'MM'-'dd",
            "\u{200e}2001\u{200e}-\u{200e}02\u{200e}-\u{200e}03",
        ),
    ] {
        wide(&mut engine, 0x10040, picture);
        for (flags, marker) in [(0x10, 0x200e), (0x20, 0x200f)] {
            let expected: Vec<u16> = expected
                .encode_utf16()
                .map(|unit| if unit == 0x200e { marker } else { unit })
                .chain([0])
                .collect();
            let required = engine
                .execute_nls_call(
                    NlsApi32::GetDateFormatW,
                    &[0x409, flags, 0x10000, 0x10040, 0, 0],
                )
                .unwrap();
            assert_eq!(required as usize, expected.len(), "{picture}/{flags:#x}");
            let length = call(
                &mut engine,
                NlsApi32::GetDateFormatW,
                &[0x409, flags, 0x10000, 0x10040, 0x11000, 128],
            );
            assert_eq!(length, required);
            let mut bytes = vec![0; length as usize * 2];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x11000), &mut bytes)
                .unwrap();
            assert_eq!(
                bytes,
                expected
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            );
        }
    }
    assert_eq!(
        engine.execute_nls_call(
            NlsApi32::GetDateFormatW,
            &[0x409, 0x80000010, 0x10000, 0x10040, 0x11000, 128],
        ),
        Err(NlsError32::InvalidFlags)
    );
}

#[test]
fn native_ansi_explicit_layout_validation_differs_from_automatic_layout_encoding() {
    let mut engine = engine();
    date(&mut engine);
    write(&mut engine, 0x10040, b"yyyy/MM/dd\0");
    write(&mut engine, 0x11000, &[0x55; 32]);
    let version = engine.memory().unwrap().transaction_version();
    for locale in [0x409, 0x411, 0x7f] {
        for flags in [0x10, 0x20] {
            assert_eq!(
                engine.execute_nls_call(
                    NlsApi32::GetDateFormatA,
                    &[locale, flags, 0x10000, 0x10040, 0x11000, 32],
                ),
                Err(NlsError32::InvalidFlags),
                "{locale:#x}/{flags:#x}"
            );
            assert_eq!(read::<32>(&engine, 0x11000), [0x55; 32]);
            assert_eq!(engine.memory().unwrap().transaction_version(), version);
        }
    }
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetDateFormatA,
            &[0x409, 0x40, 0x10000, 0x10040, 0x11000, 32]
        ),
        14
    );
    assert_eq!(read::<14>(&engine, 0x11000), *b"?2001/?02/?03\0");
    for (flags, marker) in [(0x10, 0xfd), (0x20, 0xfe), (0x40, 0xfd)] {
        assert_eq!(
            call(
                &mut engine,
                NlsApi32::GetDateFormatA,
                &[0x40d, flags, 0x10000, 0x10040, 0x11000, 32],
            ),
            14
        );
        assert_eq!(
            read::<14>(&engine, 0x11000),
            [
                marker, b'2', b'0', b'0', b'1', b'/', marker, b'0', b'2', b'/', marker, b'0', b'3',
                0
            ]
        );
    }
}

#[test]
fn native_hebrew_numeric_fields_keep_full_years_punctuation_and_calendar_range() {
    let mut engine = engine();
    date(&mut engine);
    for (picture, expected) in [
        ("y", "תשס\"א"),
        ("yy", "תשס\"א"),
        ("yyy", "תשס\"א"),
        ("yyyy", "תשס\"א"),
        ("yyyyy", "תשס\"א"),
        ("M", "ה'"),
        ("MM", "ה'"),
        ("d", "י'"),
        ("dd", "י'"),
        ("gg", "C.E."),
    ] {
        wide(&mut engine, 0x10040, picture);
        for flags in [4, 0x44] {
            let marker = if flags == 4 {
                ""
            } else if picture == "gg" {
                "\u{200e}"
            } else {
                "\u{200f}"
            };
            let expected: Vec<u16> = format!("{marker}{expected}")
                .encode_utf16()
                .chain([0])
                .collect();
            let length = call(
                &mut engine,
                NlsApi32::GetDateFormatW,
                &[0x40d, flags, 0x10000, 0x10040, 0x11000, 128],
            );
            assert_eq!(length as usize, expected.len(), "{picture}/{flags:#x}");
            let mut bytes = vec![0; expected.len() * 2];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x11000), &mut bytes)
                .unwrap();
            assert_eq!(
                bytes,
                expected
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            );
        }
    }
    wide(&mut engine, 0x10040, "d");
    for (day, expected) in [
        (23, [0x05d8, 0x0022, 0x05d5, 0]),
        (24, [0x05d8, 0x0022, 0x05d6, 0]),
    ] {
        write(
            &mut engine,
            0x10000,
            &SystemTime32 {
                year: 2024,
                month: 4,
                day,
                ..Default::default()
            }
            .bytes(),
        );
        assert_eq!(
            call(
                &mut engine,
                NlsApi32::GetDateFormatW,
                &[0x40d, 4, 0x10000, 0x10040, 0x11000, 128]
            ),
            4
        );
        assert_eq!(
            read::<8>(&engine, 0x11000),
            expected
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
                .as_slice()
        );
    }
    for (year, month, day) in [(2239, 9, 30), (2240, 1, 1), (9999, 12, 31)] {
        write(
            &mut engine,
            0x10000,
            &SystemTime32 {
                year,
                month,
                day,
                ..Default::default()
            }
            .bytes(),
        );
        write(&mut engine, 0x11000, &[0x55; 32]);
        let version = engine.memory().unwrap().transaction_version();
        assert_eq!(
            engine.execute_nls_call(
                NlsApi32::GetDateFormatW,
                &[0x40d, 4, 0x10000, 0x10040, 0x11000, 128]
            ),
            Err(NlsError32::InvalidParameter)
        );
        assert_eq!(read::<32>(&engine, 0x11000), [0x55; 32]);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
    }
}

#[test]
fn native_automatic_layout_uses_selected_calendar_names_and_hebrew_rendered_fields() {
    let mut engine = engine();
    date(&mut engine);
    for (locale, flags, picture, expected) in [
        (0x409, 0x40, "'עברית'", "\u{200e}עברית"),
        (0x40d, 0x40, "'עברית'", "\u{200f}עברית"),
        (0x40d, 0x40, "'العربية'", "\u{200e}العربية"),
        (0x420, 0x40, "'ASCII'", "\u{200f}ASCII"),
        (0x420, 0x44, "'ASCII'", "\u{200e}ASCII"),
        (0x45a, 0x40, "'ASCII'", "\u{200f}ASCII"),
        (0x465, 0x40, "'ASCII'", "\u{200f}ASCII"),
    ] {
        wide(&mut engine, 0x10040, picture);
        let length = call(
            &mut engine,
            NlsApi32::GetDateFormatW,
            &[locale, flags, 0x10000, 0x10040, 0x11000, 128],
        );
        let expected: Vec<u8> = expected
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            length as usize * 2,
            expected.len(),
            "{locale:#x}/{flags:#x}/{picture}"
        );
        let mut bytes = vec![0; expected.len()];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x11000), &mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
    for (locale, alternate) in [
        (0x409, 2u32),
        (0x401, 6),
        (0xc01, 23),
        (0x420, 2),
        (0x465, 6),
    ] {
        assert_eq!(
            call(
                &mut engine,
                NlsApi32::GetLocaleInfoW,
                &[locale, 0x2000100b, 0x11000, 2]
            ),
            2
        );
        assert_eq!(read::<4>(&engine, 0x11000), alternate.to_le_bytes());
    }
}

#[test]
fn native_hebrew_auto_normalizes_embedded_marks_but_explicit_and_other_locales_preserve_them() {
    let mut engine = engine();
    date(&mut engine);
    for (locale, flags, picture, expected) in [
        (0x40d, 0x40, "'\u{05b0}'", "\u{200f}\u{05b0}"),
        (0x40d, 0x40, "'\u{fb2a}'", "\u{200e}\u{fb2a}"),
        (0x40d, 0x40, "'\u{07c0}'", "\u{200e}\u{07c0}"),
        (0x40d, 0x40, "'A\u{200f}'", "\u{200e}A\u{200e}"),
        (0x40d, 0x10, "'A\u{200f}'", "\u{200e}A\u{200f}"),
        (0x40d, 0x20, "'A\u{200f}'", "\u{200f}A\u{200f}"),
        (0x40d, 0, "'A\u{200f}'", "A\u{200f}"),
        (0x40d, 0x40, "'א\u{200e}'", "\u{200f}א\u{200f}"),
        (0x40d, 0x20, "'א\u{200e}'", "\u{200f}א\u{200e}"),
        (0x409, 0x40, "'א\u{200f}'", "\u{200e}א\u{200f}"),
    ] {
        wide(&mut engine, 0x10040, picture);
        let length = call(
            &mut engine,
            NlsApi32::GetDateFormatW,
            &[locale, flags, 0x10000, 0x10040, 0x11000, 128],
        );
        let expected = expected
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            length as usize * 2,
            expected.len(),
            "{locale:#x}/{flags:#x}/{picture}"
        );
        let mut bytes = vec![0; expected.len()];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x11000), &mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
    write(&mut engine, 0x10040, b"'\xe0\xfd'\0");
    assert_eq!(
        call(
            &mut engine,
            NlsApi32::GetDateFormatA,
            &[0x40d, 0x40, 0x10000, 0x10040, 0x11000, 128]
        ),
        4
    );
    assert_eq!(read::<4>(&engine, 0x11000), [0xfe, 0xe0, 0xfe, 0]);
}
