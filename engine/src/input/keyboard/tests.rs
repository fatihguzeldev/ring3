use super::*;

#[test]
fn extended_scan_mapping_distinguishes_modifiers_navigation_and_keypad() {
    assert_eq!(map_virtual_key(0xa3, 4), 0xe01d);
    assert_eq!(map_virtual_key(0xe01d, 3), 0xa3);
    assert_eq!(map_virtual_key(0xe01d, 1), 0x11);
    assert_eq!(virtual_key(0x48), 0x68);
    assert_eq!(virtual_key(0xc8), 0x26);
    assert_eq!(scan_code(b'A' as u32), Some(0x1e));
}
#[test]
fn layout_character_mapping_preserves_turkish_case_and_altgr_without_changing_physical_scan() {
    assert_eq!(
        character(KeyboardLayout32::TurkishQ, b'I', false, false, false, false),
        Some('ı')
    );
    assert_eq!(
        character(KeyboardLayout32::TurkishQ, 0xde, false, true, false, false),
        Some('İ')
    );
    assert_eq!(
        character(KeyboardLayout32::TurkishQ, b'Q', false, false, true, true),
        Some('@')
    );
    assert_eq!(
        character(KeyboardLayout32::Us, b'A', true, true, false, false),
        Some('a')
    );
    assert_eq!(
        character(KeyboardLayout32::Us, b'A', false, false, true, false),
        Some('\u{1}')
    );
}

#[test]
fn windows_scan_translation_keeps_pause_prefix_and_navigation_numpad_collisions_distinct_from_physical_dik()
 {
    assert_eq!(map_virtual_key(0x13, 4), 0xe11d);
    assert_eq!(map_virtual_key(0xe11d, 3), 0x13);
    assert_eq!(map_virtual_key(0xe145, 3), 0);
    assert_eq!(map_virtual_key(0x26, 4), 0x48);
    assert_eq!(map_virtual_key(0x68, 0), 0x48);
    assert_eq!(map_virtual_key(0x48, 1), 0x26);
    assert_eq!(virtual_key(0x48), 0x68);
    assert_eq!(map_virtual_key(0x2c, 4), 0x54);
}
