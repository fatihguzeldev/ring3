use super::*;
use crate::{
    memory::AddressSpace,
    process::windows::user32::{User32CallContext32, User32Context32},
};

fn combo() -> (User32Context32, AddressSpace, User32CallContext32, u32) {
    let (mut context, mut ram, call) = fixture();
    context.builtin_procedures = Some([GuestAddress(0x70000), GuestAddress(0x70020)]);
    context
        .state
        .install_standard_classes(GuestAddress(0x70020))
        .unwrap();
    let atom = context.state.standard_atom(ControlKind32::ComboBox);
    let window = create(&mut context, &mut ram, call, atom as u32);
    (context, ram, call, window)
}

fn text(ram: &mut AddressSpace, value: &str, wide: bool) -> u32 {
    let bytes: Vec<u8> = if wide {
        value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        value.bytes().chain([0]).collect()
    };
    ram.write(GuestAddress(0x10100), &bytes).unwrap();
    0x10100
}

#[test]
fn literal_combo_get_and_set_item_data_preserve_guest_pointers_without_reading_them() {
    for wide in [false, true] {
        let (mut context, mut ram, call, window) = combo();
        let pointer = text(&mut ram, "mode", wide);
        assert_eq!(
            result(
                context
                    .control_message(&[window, 0x143, 0, pointer], wide, &mut ram, call)
                    .unwrap()
            ),
            0
        );
        let pointer = 0x10300;
        ram.write(GuestAddress(pointer), &0x12345678u32.to_le_bytes())
            .unwrap();
        // CB_SETITEMDATA accepts the application value, and CB_GETITEMDATA ignores lParam.
        assert_eq!(
            result(
                context
                    .control_message(&[window, 0x151, 0, pointer], wide, &mut ram, call)
                    .unwrap()
            ),
            0
        );
        for ignored in [0, u32::MAX] {
            assert_eq!(
                result(
                    context
                        .control_message(&[window, 0x150, 0, ignored], wide, &mut ram, call)
                        .unwrap()
                ),
                pointer
            );
        }
        assert_eq!(
            result(
                context
                    .control_message(&[window, 0x151, 1, 0xdeadbeef], wide, &mut ram, call)
                    .unwrap()
            ),
            u32::MAX
        );
        assert_eq!(
            result(
                context
                    .control_message(&[window, 0x150, 1, 0], wide, &mut ram, call)
                    .unwrap()
            ),
            u32::MAX
        );
        assert_eq!(
            result(
                context
                    .control_message(&[window, 0x150, 0, 0], wide, &mut ram, call)
                    .unwrap()
            ),
            pointer
        );
        let mut bytes = [0; 4];
        ram.read(GuestAddress(pointer), &mut bytes).unwrap();
        assert_eq!(u32::from_le_bytes(bytes), 0x12345678);
    }
}

#[test]
fn literal_combo_item_data_follows_insert_delete_and_reset() {
    let (mut context, mut ram, call, window) = combo();
    let pointer = text(&mut ram, "last", false);
    context
        .control_message(&[window, 0x143, 0, pointer], false, &mut ram, call)
        .unwrap();
    context
        .control_message(&[window, 0x151, 0, 0xfeedbeef], false, &mut ram, call)
        .unwrap();
    let pointer = text(&mut ram, "first", false);
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x14a, 0, pointer], false, &mut ram, call)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x150, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x150, 1, 0], false, &mut ram, call)
                .unwrap()
        ),
        0xfeedbeef
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x144, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        1
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x150, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        0xfeedbeef
    );
    context
        .control_message(&[window, 0x14b, 0, 0], false, &mut ram, call)
        .unwrap();
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x146, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x150, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        u32::MAX
    );
}

#[test]
fn literal_combo_exact_search_is_case_insensitive_wraps_and_never_reads_item_data() {
    for wide in [false, true] {
        let (mut context, mut ram, call, window) = combo();
        for (index, value) in ["Alpha", "alphabet", "ALPHA", "beta", "a-b", "ab"]
            .into_iter()
            .enumerate()
        {
            let pointer = text(&mut ram, value, wide);
            assert_eq!(
                result(
                    context
                        .control_message(&[window, 0x143, 0, pointer], wide, &mut ram, call)
                        .unwrap()
                ),
                index as u32
            );
            context
                .control_message(
                    &[window, 0x151, index as u32, 0x1000 + index as u32],
                    wide,
                    &mut ram,
                    call,
                )
                .unwrap();
        }
        for (query, start, expected) in [
            ("alpha", u32::MAX, 0),
            ("alpha", 0, 2),
            ("alpha", 2, 0),
            ("BETA", 3, 3),
            ("alph", u32::MAX, u32::MAX),
            ("missing", u32::MAX, u32::MAX),
            ("ab", u32::MAX, 5),
            ("alpha", 6, u32::MAX),
        ] {
            let pointer = text(&mut ram, query, wide);
            assert_eq!(
                result(
                    context
                        .control_message(&[window, 0x158, start, pointer], wide, &mut ram, call)
                        .unwrap()
                ),
                expected,
                "{query} after {start}"
            );
        }
        for index in 0..6 {
            assert_eq!(
                result(
                    context
                        .control_message(&[window, 0x150, index, 0], wide, &mut ram, call)
                        .unwrap()
                ),
                0x1000 + index
            );
        }
    }
}

#[test]
fn exact_search_uses_full_unicode_comparison_and_rejects_empty_queries() {
    let (mut context, mut ram, call, window) = combo();
    for value in ["", "é"] {
        let pointer = text(&mut ram, value, true);
        context
            .control_message(&[window, 0x143, 0, pointer], true, &mut ram, call)
            .unwrap();
    }
    // Canonically equivalent strings have different UTF-16 lengths, but compare equal.
    for (query, expected) in [("E\u{301}", 1), ("", u32::MAX)] {
        let pointer = text(&mut ram, query, true);
        assert_eq!(
            result(
                context
                    .control_message(&[window, 0x158, u32::MAX, pointer], true, &mut ram, call)
                    .unwrap()
            ),
            expected
        );
    }
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x158, u32::MAX, 0], true, &mut ram, call)
                .unwrap()
        ),
        u32::MAX
    );
}
