use super::*;
use crate::{
    input::{keyboard::KeyboardLayout32, source::SourceSnapshot32},
    memory::MemoryError,
};

#[test]
fn asynchronous_recent_bits_are_independent_of_dequeue_and_physical_mouse_mapping() {
    let (mut context, mut memory, call, _) = focused();
    let mut snapshot = SourceSnapshot32 {
        focused: true,
        ..Default::default()
    };
    snapshot.keys[0x1e] = true;
    snapshot.presses[0x1e] = 1;
    snapshot.buttons[0] = true;
    snapshot.button_presses[0] = 1;
    context.input.observe_async(&snapshot);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetAsyncKeyState,
            &[65]
        ),
        0xffff8001
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetAsyncKeyState,
            &[65]
        ),
        0xffff8000
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[65]
        ),
        0
    );
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::SwapMouseButton,
        &[1],
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetAsyncKeyState,
            &[1]
        ),
        0xffff8001
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetAsyncKeyState,
            &[2]
        ),
        0
    );
    snapshot.focused = false;
    snapshot.keys[0x1e] = false;
    context.input.observe_async(&snapshot);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetAsyncKeyState,
            &[65]
        ),
        0
    );
}

#[test]
fn filtered_hardware_dequeue_publishes_its_keyboard_snapshot_but_fake_posts_do_not() {
    let (mut context, mut memory, call, window) = focused();
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PostMessageW,
        &[window, 0x100, 0x10, 0],
    );
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0x100, 0x100, 1],
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0x10]
        ),
        0
    );
    key(&mut context, 1, 0x1d, true);
    context
        .deliver_input(
            event(
                2,
                InputKind::Button {
                    button: 0,
                    pressed: true,
                },
            ),
            0,
        )
        .unwrap();
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0x201, 0x201, 0]
        ),
        1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0x11]
        ),
        0
    );
    assert!(matches!(
        context.prepare_arguments(
            User32Api32::PeekMessageW,
            &[0x30000, window, 0x201, 0x201, 1],
            &mut memory,
            call
        ),
        Err(User32Error32::Memory(_))
    ));
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0x11]
        ),
        0
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0x201, 0x201, 1]
        ),
        1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0x11]
        ),
        0xffff8000
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[1]
        ),
        0xffff8000
    );
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::GetKeyboardState,
        &[0x10300],
    );
    let mut bytes = [0; 256];
    memory.read(GuestAddress(0x10300), &mut bytes).unwrap();
    assert_eq!(bytes[0xa2], 0x80);
    assert_eq!(bytes[0x11], 0x80);
}

#[test]
fn repeat_and_extended_bits_preserve_toggle_and_left_right_identity() {
    let (mut context, mut memory, call, window) = focused();
    key(&mut context, 1, 0x3a, true);
    key(&mut context, 2, 0x3a, true);
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0, 0, 1],
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0x14]
        ),
        0xffff8001
    );
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0, 0, 1],
    );
    let repeated = super::super::super::memory::message(&memory, 0x10200).unwrap();
    assert_ne!(repeated.lparam & (1 << 30), 0);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0x14]
        ),
        0xffff8001
    );
    key(&mut context, 3, 0x9d, true);
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0, 0, 1],
    );
    let message = super::super::super::memory::message(&memory, 0x10200).unwrap();
    assert_eq!(message.wparam, 0x11);
    assert_ne!(message.lparam & (1 << 24), 0);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0xa3]
        ),
        0xffff8000
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[0xa2]
        ),
        0
    );
}

#[test]
fn turkish_layout_dead_keys_and_output_failure_have_transactional_translation_state() {
    let (mut context, mut memory, call, _) = focused();
    memory.write(GuestAddress(0x10100), b"0000041F\0").unwrap();
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::LoadKeyboardLayoutA,
            &[0x10100, 1]
        ),
        KeyboardLayout32::TurkishQ.handle()
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::MapVirtualKeyExW,
            &[0x0c, 3, 0x041f041f]
        ),
        0xdf
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::VkKeyScanExW,
            &['İ' as u32, 0x041f041f]
        ),
        0x1de
    );
    let mut keys = [0; 256];
    keys[0x10] = 0x80;
    memory.write(GuestAddress(0x10300), &keys).unwrap();
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ToUnicodeEx,
            &[b'3' as u32, 4, 0x10300, 0x10400, 2, 4, 0x041f041f]
        ),
        u32::MAX
    );
    assert_eq!(context.input.dead, None);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ToUnicode,
            &[b'3' as u32, 4, 0x10300, 0x10400, 2, 0]
        ),
        u32::MAX
    );
    assert_eq!(context.input.dead, Some('^'));
    keys[0x10] = 0;
    memory.write(GuestAddress(0x10300), &keys).unwrap();
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ToUnicode,
            &[b'A' as u32, 0x1e, 0x10300, 0x10400, 2, 0]
        ),
        1
    );
    let mut bytes = [0; 2];
    memory.read(GuestAddress(0x10400), &mut bytes).unwrap();
    assert_eq!(u16::from_le_bytes(bytes), 'â' as u16);
    context.input.dead = Some('^');
    memory.exhaust_versions_for_test();
    assert_eq!(
        context.prepare_arguments(
            User32Api32::ToUnicode,
            &[b'A' as u32, 0x1e, 0x10300, 0x10402, 2, 0],
            &mut memory,
            call
        ),
        Err(User32Error32::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(context.input.dead, Some('^'));
}

#[test]
fn toascii_packs_dead_key_fallback_in_one_word_and_preserves_adjacent_canary() {
    let (mut context, mut memory, call, _) = focused();
    memory.write(GuestAddress(0x10300), &[0; 256]).unwrap();
    memory.write(GuestAddress(0x10400), &[0xcc; 4]).unwrap();
    context.input.dead = Some('^');
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ToAscii,
            &[b'B' as u32, 0x30, 0x10300, 0x10400, 0]
        ),
        2
    );
    let mut bytes = [0; 4];
    memory.read(GuestAddress(0x10400), &mut bytes).unwrap();
    assert_eq!(bytes, [b'^', b'b', 0xcc, 0xcc]);
    assert_eq!(context.input.dead, None);
}
