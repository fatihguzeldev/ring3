use super::*;
use crate::windows::user32::menus::MenuItem32;

fn item(id: u32, text: &str) -> MenuItem32 {
    MenuItem32::command(id, text.encode_utf16().collect())
}

fn fixture() -> (MenuStore32, MenuHandle32, MenuHandle32) {
    let mut menus = MenuStore32::new(Default::default()).unwrap();
    let root = menus.create(true).unwrap();
    let submenu = menus.create(true).unwrap();
    menus
        .insert(
            root,
            0,
            MenuItem32 {
                kind: 0x800,
                ..item(0, "")
            },
        )
        .unwrap();
    menus.insert(root, 1, item(1, "&Open")).unwrap();
    menus
        .insert(
            root,
            2,
            MenuItem32 {
                state: 3,
                ..item(2, "&Off")
            },
        )
        .unwrap();
    menus.insert(root, 3, item(3, "&Other")).unwrap();
    menus
        .insert(
            root,
            4,
            MenuItem32 {
                submenu: Some(submenu),
                ..item(4, "&Nested")
            },
        )
        .unwrap();
    menus.insert(submenu, 0, item(0x12345678, "&Leaf")).unwrap();
    (menus, root, submenu)
}

#[test]
fn keyboard_selection_skips_separators_and_disabled_items_cannot_execute() {
    let (menus, root, _) = fixture();
    let mut track = MenuTracking32::new(&menus, root).unwrap();
    assert_eq!(
        track.input(&menus, MenuInput32::Next(false)).unwrap(),
        MenuAction32::Select {
            depth: 0,
            position: Some(1)
        }
    );
    assert_eq!(
        track.input(&menus, MenuInput32::Next(false)).unwrap(),
        MenuAction32::Select {
            depth: 0,
            position: Some(2)
        }
    );
    assert_eq!(
        track.input(&menus, MenuInput32::Open).unwrap(),
        MenuAction32::None
    );
    assert_eq!(
        track.input(&menus, MenuInput32::Home).unwrap(),
        MenuAction32::Select {
            depth: 0,
            position: Some(1)
        }
    );
    assert_eq!(
        track.input(&menus, MenuInput32::End).unwrap(),
        MenuAction32::Select {
            depth: 0,
            position: Some(4)
        }
    );
}

#[test]
fn duplicate_mnemonics_cycle_while_a_unique_submenu_keeps_full_command_identity() {
    let (menus, root, submenu) = fixture();
    let mut track = MenuTracking32::new(&menus, root).unwrap();
    assert_eq!(
        track
            .input(&menus, MenuInput32::Mnemonic(b'o' as u16))
            .unwrap(),
        MenuAction32::Select {
            depth: 0,
            position: Some(1)
        }
    );
    assert_eq!(
        track
            .input(&menus, MenuInput32::Mnemonic(b'O' as u16))
            .unwrap(),
        MenuAction32::Select {
            depth: 0,
            position: Some(3)
        }
    );
    assert_eq!(
        track
            .input(&menus, MenuInput32::Mnemonic(b'n' as u16))
            .unwrap(),
        MenuAction32::Open {
            depth: 1,
            menu: submenu,
            parent_position: 4
        }
    );
    assert_eq!(
        track.input(&menus, MenuInput32::Next(false)).unwrap(),
        MenuAction32::Select {
            depth: 1,
            position: Some(0)
        }
    );
    assert_eq!(
        track.input(&menus, MenuInput32::Open).unwrap(),
        MenuAction32::Command {
            menu: submenu,
            position: 0,
            id: 0x12345678
        }
    );
    assert_eq!(
        track.input(&menus, MenuInput32::Open),
        Err(E::InvalidRequest)
    );
}

#[test]
fn leaving_a_submenu_keeps_parent_selection_and_stale_menu_handles_are_rejected() {
    let (mut menus, root, submenu) = fixture();
    let mut track = MenuTracking32::new(&menus, root).unwrap();
    track
        .input(&menus, MenuInput32::Mnemonic(b'n' as u16))
        .unwrap();
    assert_eq!(
        track.input(&menus, MenuInput32::Back).unwrap(),
        MenuAction32::Close { depth: 1 }
    );
    assert_eq!(track.level(0).unwrap().selected, Some(4));
    track.input(&menus, MenuInput32::Open).unwrap();
    menus.destroy(submenu).unwrap();
    assert_eq!(track.input(&menus, MenuInput32::Open), Err(E::Win32(1401)));
    assert_eq!(track.level(1).unwrap().menu, submenu);
}

#[test]
fn escaped_ampersands_are_never_access_keys_and_unmatched_characters_are_forwarded() {
    assert_eq!(
        mnemonic(&"save && exit".encode_utf16().collect::<Vec<_>>()),
        None
    );
    assert_eq!(
        mnemonic(&"save && &exit\tCtrl+X".encode_utf16().collect::<Vec<_>>()),
        Some(b'e' as u16)
    );
    let (menus, root, _) = fixture();
    let mut track = MenuTracking32::new(&menus, root).unwrap();
    assert_eq!(
        track
            .input(&menus, MenuInput32::Mnemonic(b'z' as u16))
            .unwrap(),
        MenuAction32::Unmatched(b'z' as u16)
    );
    assert_eq!(track.level(0).unwrap().selected, None);
}
