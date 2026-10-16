use super::*;
use crate::windows::user32::dialogs::DialogItem32;

fn template() -> DialogTemplate32 {
    DialogTemplate32 {
        extended: false,
        help: 0x11223344,
        style: 0x90000000,
        ex_style: 0x80,
        units: DialogUnits32 {
            x: -4,
            y: 8,
            width: 4,
            height: 8,
        },
        menu: DialogName32::Empty,
        class: DialogName32::Empty,
        title: "é".encode_utf16().collect(),
        font: None,
        items: vec![DialogItem32 {
            help: 0,
            style: 0x10010000,
            ex_style: 0,
            units: DialogUnits32 {
                x: 4,
                y: -8,
                width: 8,
                height: 16,
            },
            id: 100,
            class: DialogName32::Ordinal(0x84),
            title: DialogName32::Text("child".encode_utf16().collect()),
            creation: vec![6, 0, 0x34, 0x12, 0x78, 0x56],
        }],
        length: 100,
    }
}

#[test]
fn template_arguments_use_real_class_names_signed_dialog_units_and_original_creation_words() {
    let prepared = Prepared32::new(
        template(),
        Point32 { x: 7, y: 15 },
        true,
        CodePage32::Windows1252,
        4096,
    )
    .unwrap();
    let root = prepared.windows[0].arguments(0x30000, 0x400000, 9).unwrap();
    assert_eq!(
        &root[3..11],
        &[0x80000000, (-7i32) as u32, 15, 7, 15, 9, 0, 0x400000]
    );
    assert_eq!(
        &prepared.bytes[32..46],
        &[35, 0, 51, 0, 50, 0, 55, 0, 55, 0, 48, 0, 0, 0]
    );
    let child = prepared.windows[1]
        .arguments(0x30000, 0x400000, 11)
        .unwrap();
    assert_eq!(
        &child[3..11],
        &[0x50010000, 7, (-15i32) as u32, 14, 30, 11, 100, 0x400000]
    );
    let creation = (child[11] - 0x30000) as usize;
    assert_eq!(
        &prepared.bytes[creation..creation + 6],
        &[6, 0, 0x34, 0x12, 0x78, 0x56]
    );
    let class = (child[1] - 0x30000) as usize;
    assert_eq!(
        &prepared.bytes[class..class + 20],
        b"S\0c\0r\0o\0l\0l\0B\0a\0r\0\0\0"
    );
    assert!(prepared.visible);
}

#[test]
fn ansi_template_strings_use_the_configured_multibyte_codepage() {
    let prepared = Prepared32::new(
        template(),
        Point32 { x: 8, y: 16 },
        false,
        CodePage32::Utf8,
        4096,
    )
    .unwrap();
    let root = prepared.windows[0].arguments(0x30000, 1, 0).unwrap();
    let title = (root[2] - 0x30000) as usize;
    assert_eq!(&prepared.bytes[title..title + 3], &[0xc3, 0xa9, 0]);
    let child = prepared.windows[1].arguments(0x30000, 1, 2).unwrap();
    let class = (child[1] - 0x30000) as usize;
    assert_eq!(&prepared.bytes[class..class + 10], b"ScrollBar\0");
}

#[test]
fn exact_prepared_byte_budget_and_signed_halfway_rounding_are_preserved() {
    let prepared = Prepared32::new(
        template(),
        Point32 { x: 7, y: 15 },
        true,
        CodePage32::Windows1252,
        4096,
    )
    .unwrap();
    let count = prepared.bytes.len();
    assert!(
        Prepared32::new(
            template(),
            Point32 { x: 7, y: 15 },
            true,
            CodePage32::Windows1252,
            count
        )
        .is_ok()
    );
    assert!(matches!(
        Prepared32::new(
            template(),
            Point32 { x: 7, y: 15 },
            true,
            CodePage32::Windows1252,
            count - 1
        ),
        Err(E::Capacity)
    ));
    for (input, expected) in [(-3, -2), (-1, -1), (1, 1), (3, 2)] {
        assert_eq!(mul_div(input, 1, 2).unwrap(), expected);
    }
    assert_eq!(mul_div(i32::MAX, i32::MAX, 1), Err(E::Win32(87)));
}

#[test]
fn native_dialog_font_attributes_are_normalized_before_gdi_creation() {
    let font = logical_font(
        &DialogFont32 {
            points: 9,
            weight: 700,
            italic: true,
            charset: 1,
            face: "Arial".encode_utf16().collect(),
        },
        Point32 { x: 96, y: 96 },
    )
    .unwrap();
    assert_eq!(
        (font.height, font.weight, font.italic, font.charset),
        (-12, 400, true, 0)
    );
    assert_eq!(font.face, "Arial".encode_utf16().collect::<Vec<_>>());
}
