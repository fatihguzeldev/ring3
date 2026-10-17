use super::*;

#[test]
fn invalid_modifier_rules_add_defaults_without_losing_entered_or_extended_modifiers() {
    let mut key = Hotkey32 {
        invalid: 1 | 2,
        defaults: 4,
        ..Default::default()
    };
    key.key_down(65, false);
    assert_eq!(key.key, 0x441);
    key.key_down(0x10, false);
    key.key_down(66, true);
    assert_eq!(key.key, 0xd42);
    key.key_up(0x10);
    assert_eq!(key.key, 0xd42);
    assert_eq!(key.modifiers, 0);
    assert_eq!(
        String::from_utf16(&key.label(crate::input::keyboard::KeyboardLayout32::Us)).unwrap(),
        "Shift + Alt + B"
    );
    key.key_down(8, false);
    assert_eq!(key.key, 0);
}

#[test]
fn every_modifier_combination_maps_to_its_documented_invalid_flag() {
    for (modifiers, invalid) in [1, 2, 4, 16, 8, 32, 64, 128].into_iter().enumerate() {
        let mut key = Hotkey32 {
            modifiers: modifiers as u8,
            invalid,
            defaults: 7,
            ..Default::default()
        };
        key.key_down(65, false);
        assert_eq!(key.key, 0x741);
    }
}
