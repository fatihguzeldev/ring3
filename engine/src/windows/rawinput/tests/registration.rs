use crate::windows::{
    rawinput::{RawInputRuntime32, RawRegistration32},
    user32::User32Error32,
};
fn row(usage: u16, flags: u32, window: u32) -> RawRegistration32 {
    RawRegistration32 {
        page: 1,
        usage,
        flags,
        window,
    }
}
#[test]
fn registration_batch_is_atomic_and_page_exclusion_differs_from_no_legacy() {
    let mut runtime = RawInputRuntime32::new(8).unwrap();
    let rows = runtime.prepare_registrations(&[row(6, 0x30, 7)]).unwrap();
    runtime.commit_registrations(rows);
    assert!(runtime.no_legacy(true));
    assert_eq!(
        runtime
            .prepare_registrations(&[row(2, 0, 7), row(6, 1, 7)])
            .err(),
        Some(User32Error32::Win32(87))
    );
    assert_eq!(runtime.registrations(), &[row(6, 0x30, 7)]);
    let rows = runtime
        .prepare_registrations(&[row(6, 1, 0), row(0, 0x20, 7), row(6, 0x10, 0)])
        .unwrap();
    runtime.commit_registrations(rows);
    assert!(!runtime.no_legacy(true));
    assert!(
        runtime
            .prepare_packet(
                crate::input::InputKind::Key {
                    code: 0x1e,
                    pressed: true
                },
                65,
                false,
                7,
                true,
                |_| Some(3)
            )
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .prepare_packet(
                crate::input::InputKind::Button {
                    button: 0,
                    pressed: true
                },
                0,
                false,
                7,
                true,
                |_| Some(3)
            )
            .unwrap()
            .is_some()
    );
}
