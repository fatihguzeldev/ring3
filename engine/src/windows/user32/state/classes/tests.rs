use super::*;
use crate::memory::GuestAddress;

fn state() -> User32State32 {
    User32State32::new(User32Limits32 {
        classes: 1,
        windows: 8,
        messages: 8,
        continuations: 4,
    })
    .unwrap()
}

fn class(name: &str, procedure: u32) -> WindowClass32 {
    WindowClass32 {
        name: name.encode_utf16().collect(),
        procedure: GuestAddress(procedure),
        unicode: true,
        ..Default::default()
    }
}

#[test]
fn all_system_classes_share_the_atom_issuer_without_consuming_guest_class_quota() {
    let mut state = state();
    state
        .install_standard_classes(GuestAddress(0x1000))
        .unwrap();
    let mut dialog = class("#32770", 0x1020);
    dialog.extra_bytes = 30;
    let atom = state.install_system_class(dialog.clone()).unwrap();
    assert!(state.is_system_class(atom));
    assert_eq!(state.standard_kind(atom), None);
    let before = state.next_atom;
    assert_eq!(state.install_system_class(dialog).unwrap(), atom);
    assert_eq!(state.next_atom, before);
    assert_eq!(state.system_atoms.len(), 7);
    assert_eq!(
        state.unregister_class(0, &[], Some(atom)),
        Err(User32Error32::Win32(1411))
    );
    assert!(state.register_class(class("guest", 0x2000)).unwrap() > atom);
    assert_eq!(
        state.register_class(class("second", 0x2000)),
        Err(User32Error32::Capacity)
    );
    assert_eq!(state.classes.len(), 8);
}

#[test]
fn system_class_collision_and_atom_exhaustion_leave_the_whole_standard_group_unpublished() {
    for collision in [false, true] {
        let mut state = state();
        if collision {
            state.register_class(class("Button", 0x2000)).unwrap();
        } else {
            state.next_atom = u16::MAX - 3;
        }
        let before = state.next_atom;
        let expected = if collision {
            User32Error32::Win32(1410)
        } else {
            User32Error32::Capacity
        };
        assert_eq!(
            state.install_standard_classes(GuestAddress(0x1000)),
            Err(expected)
        );
        assert_eq!(state.next_atom, before);
        assert_eq!(state.classes.len(), usize::from(collision));
        assert!(state.system_atoms.is_empty());
        assert_eq!(state.standard_atoms, [0; 6]);
    }
}
