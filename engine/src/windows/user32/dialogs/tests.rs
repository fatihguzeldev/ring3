use super::super::User32Error32 as E;
use super::*;

fn words(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn standard() -> Vec<u8> {
    let mut bytes = words(&[
        0x0040, 0x90c8, 0, 0, 2, 0xfff6, 3, 120, 80, 0xffff, 7, 0, 0x0044, 0, 9, 0x004e, 0,
    ]);
    bytes.extend_from_slice(&[0xcc; 2]);
    bytes.extend(words(&[
        3, 0x5001, 0, 0, 1, 2, 40, 12, 42, 0xffff, 0x80, 0x004f, 0x004b, 0, 6, 0x1234, 0x5678,
    ]));
    bytes.extend_from_slice(&[0xcc; 2]);
    bytes.extend(words(&[
        0, 0x5000, 0, 0, 10, 20, 30, 40, 0xffff, 0xffff, 0x82, 0xffff, 3, 0,
    ]));
    bytes
}

fn extended() -> Vec<u8> {
    let mut bytes = words(&[
        1, 0xffff, 0xbeef, 0xcafe, 0x100, 0, 0x40, 0x90c8, 1, 0, 0, 50, 25, 0, 0, 0x0045, 0, 10,
        700, 0x0201, 0x004d, 0x006f, 0x006e, 0x006f, 0,
    ]);
    bytes.extend_from_slice(&[0xcc; 2]);
    bytes.extend(words(&[
        0x1234, 0, 0, 0, 0, 0x5000, 0xfffe, 4, 15, 6, 0x5678, 0x1234, 0xffff, 0x85, 0xffff, 0x10,
        4, 0xaabb, 0xccdd,
    ]));
    bytes
}

#[test]
fn standard_literal_offsets_keep_ordinals_signed_units_and_creation_size_word() {
    let bytes = standard();
    assert_eq!(bytes.len(), 100);
    let dialog = DialogTemplate32::parse(&bytes, 2).unwrap();
    assert!(!dialog.extended);
    assert_eq!(dialog.style, 0x90c80040);
    assert_eq!(dialog.ex_style, 0);
    assert_eq!(
        dialog.units,
        DialogUnits32 {
            x: -10,
            y: 3,
            width: 120,
            height: 80
        }
    );
    assert_eq!(dialog.menu, DialogName32::Ordinal(7));
    assert_eq!(dialog.class, DialogName32::Empty);
    assert_eq!(dialog.title, [0x44]);
    assert_eq!(
        dialog.font,
        Some(DialogFont32 {
            points: 9,
            weight: 0,
            italic: false,
            charset: 0,
            face: vec![0x4e]
        })
    );
    assert_eq!(dialog.items.len(), 2);
    assert_eq!(dialog.items[0].id, 42);
    assert_eq!(dialog.items[0].class, DialogName32::Ordinal(0x80));
    assert_eq!(dialog.items[0].title, DialogName32::Text(vec![0x4f, 0x4b]));
    assert_eq!(dialog.items[0].creation, [6, 0, 0x34, 0x12, 0x78, 0x56]);
    assert_eq!(dialog.items[1].id, 0xffff);
    assert_eq!(dialog.items[1].class, DialogName32::Ordinal(0x82));
    assert_eq!(dialog.items[1].title, DialogName32::Ordinal(3));
    assert!(dialog.items[1].creation.is_empty());
    assert_eq!(dialog.length, 100);
}

#[test]
fn extended_literal_offsets_keep_help_full_control_ids_and_native_creation_size_word() {
    let bytes = extended();
    assert_eq!(bytes.len(), 90);
    let dialog = DialogTemplate32::parse(&bytes, 1).unwrap();
    assert!(dialog.extended);
    assert_eq!(dialog.help, 0xcafebeef);
    assert_eq!(dialog.ex_style, 0x100);
    assert_eq!(
        dialog.font,
        Some(DialogFont32 {
            points: 10,
            weight: 700,
            italic: true,
            charset: 2,
            face: "Mono".encode_utf16().collect()
        })
    );
    assert_eq!(dialog.items[0].help, 0x1234);
    assert_eq!(dialog.items[0].id, 0x12345678);
    assert_eq!(dialog.items[0].units.x, -2);
    assert_eq!(dialog.items[0].class, DialogName32::Ordinal(0x85));
    assert_eq!(dialog.items[0].creation, [4, 0, 0xbb, 0xaa, 0xdd, 0xcc]);
    assert_eq!(dialog.length, 90);
}

#[test]
fn all_truncations_invalid_version_and_item_limits_fail_before_publication() {
    for bytes in [standard(), extended()] {
        for length in 0..bytes.len() {
            assert_eq!(
                DialogTemplate32::parse(&bytes[..length], 2),
                Err(E::InvalidRequest),
                "cut {length}"
            );
        }
        assert_eq!(DialogTemplate32::parse(&bytes, 0), Err(E::Capacity));
    }
    let mut bytes = extended();
    bytes[..2].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(DialogTemplate32::parse(&bytes, 2), Err(E::InvalidRequest));
    assert_eq!(
        DialogTemplate32::parse(&words(&[0, 0, 0, 0, 0xffff]), 1024),
        Err(E::Capacity)
    );
}

#[test]
fn a_bounded_reader_only_touches_the_actual_template_and_preserves_utf16_units() {
    let mut bytes = words(&[0, 0, 0, 0, 0, 0, 0, 10, 10, 0, 0, 0xd800, 0]);
    bytes.extend_from_slice(&[0xcc; 8]);
    let mut last = 0;
    let dialog = DialogTemplate32::read(
        |at, output| {
            last = at + output.len();
            output.copy_from_slice(&bytes[at..last]);
            Ok(())
        },
        26,
        0,
    )
    .unwrap();
    assert_eq!(dialog.title, [0xd800]);
    assert_eq!(dialog.length, 26);
    assert_eq!(last, 26);
}
