use crate::{
    input::InputKind,
    windows::rawinput::{KEYBOARD, MOUSE, RawInputRuntime32, RawRegistration32},
};
fn registered(limit: usize, usage: u16) -> RawInputRuntime32 {
    let mut runtime = RawInputRuntime32::new(limit).unwrap();
    let prepared = runtime
        .prepare_registrations(&[RawRegistration32 {
            page: 1,
            usage,
            flags: 0,
            window: 7,
        }])
        .unwrap();
    runtime.commit_registrations(prepared);
    runtime
}
#[test]
fn literal_keyboard_and_mouse_bytes_preserve_flags_and_signed_deltas() {
    let mut keyboard = registered(2, 6);
    let packet = keyboard
        .prepare_packet(
            InputKind::Key {
                code: 0x9d,
                pressed: false,
            },
            0x11,
            false,
            7,
            true,
            |window| (window == 7).then_some(3),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        &packet.packet().bytes[..16],
        &[1, 0, 0, 0, 32, 0, 0, 0, 2, 0, 0, 0x59, 0, 0, 0, 0]
    );
    assert_eq!(
        &packet.packet().bytes[16..32],
        &[0x1d, 0, 3, 0, 0, 0, 0x11, 0, 1, 1, 0, 0, 0, 0, 0, 0]
    );
    let id = packet.packet().id;
    keyboard.commit_packet(packet);
    assert_eq!(
        keyboard.packet(id, 4).err(),
        Some(crate::windows::user32::User32Error32::Win32(6))
    );
    assert_eq!(
        u32::from_le_bytes(
            keyboard.packet(id, 3).unwrap().bytes[8..12]
                .try_into()
                .unwrap()
        ),
        KEYBOARD
    );
    let mouse = registered(2, 2);
    let packet = mouse
        .prepare_packet(
            InputKind::Relative {
                dx: -7,
                dy: 9,
                wheel: -120,
            },
            0,
            false,
            7,
            true,
            |window| (window == 7).then_some(3),
        )
        .unwrap()
        .unwrap();
    let bytes = &packet.packet().bytes;
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), MOUSE);
    assert_eq!(&bytes[20..24], &[0, 4, 0x88, 0xff]);
    assert_eq!(&bytes[28..36], &[0xf9, 0xff, 0xff, 0xff, 9, 0, 0, 0]);
}
#[test]
fn packet_capacity_has_no_identity_side_effect_and_focus_cannot_be_granted() {
    let mut runtime = registered(1, 6);
    let kind = InputKind::Key {
        code: 0x1e,
        pressed: true,
    };
    assert!(
        runtime
            .prepare_packet(kind, 65, false, 7, false, |_| Some(3))
            .unwrap()
            .is_none()
    );
    let packet = runtime
        .prepare_packet(kind, 65, false, 7, true, |_| Some(3))
        .unwrap()
        .unwrap();
    let first = packet.packet().id;
    runtime.commit_packet(packet);
    assert!(
        runtime
            .prepare_packet(kind, 65, false, 7, true, |_| Some(3))
            .is_err()
    );
    runtime.retire(&[first]);
    let next = runtime
        .prepare_packet(kind, 65, false, 7, true, |_| Some(3))
        .unwrap()
        .unwrap();
    assert_eq!(next.packet().id, first + 4);
}

#[test]
fn another_thread_read_does_not_reclaim_a_parked_background_packet() {
    let mut runtime = RawInputRuntime32::new(4).unwrap();
    let rows = runtime
        .prepare_registrations(&[RawRegistration32 {
            page: 1,
            usage: 6,
            flags: 0x100,
            window: 7,
        }])
        .unwrap();
    runtime.commit_registrations(rows);
    let kind = InputKind::Key {
        code: 0x1e,
        pressed: true,
    };
    let first = runtime
        .prepare_packet(kind, 65, false, 0, true, |window| {
            (window == 7).then_some(3)
        })
        .unwrap()
        .unwrap();
    let first_id = first.packet().id;
    runtime.commit_packet(first);
    runtime.mark_delivered(3, first_id);
    let rows = runtime
        .prepare_registrations(&[RawRegistration32 {
            page: 1,
            usage: 6,
            flags: 0x100,
            window: 8,
        }])
        .unwrap();
    runtime.commit_registrations(rows);
    let second = runtime
        .prepare_packet(kind, 65, false, 0, true, |window| {
            (window == 8).then_some(4)
        })
        .unwrap()
        .unwrap();
    let second_id = second.packet().id;
    runtime.commit_packet(second);
    runtime.mark_delivered(4, second_id);
    assert!(runtime.packet(first_id, 3).is_ok());
    runtime.mark_delivered(3, 0);
    assert!(runtime.packet(first_id, 3).is_err());
    assert!(runtime.packet(second_id, 4).is_ok());
}
