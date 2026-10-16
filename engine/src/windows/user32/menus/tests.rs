use super::*;
use crate::windows::user32::User32Error32 as E;

fn limits() -> MenuLimits32 {
    MenuLimits32 {
        menus: 4,
        items: 8,
        text_units: 12,
        depth: 3,
    }
}

fn item(id: u32, text: &str) -> MenuItem32 {
    MenuItem32::command(id, text.encode_utf16().collect())
}

fn popup(child: MenuHandle32) -> MenuItem32 {
    MenuItem32 {
        submenu: Some(child),
        ..item(0, "p")
    }
}

#[test]
fn loading_a_resource_creates_real_submenu_identity_and_recursive_lifetime() {
    let bytes: Vec<u8> = [0u16, 0, 0x90, b'F' as u16, 0, 0x80, 41, b'X' as u16, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut menus = MenuStore32::new(limits()).unwrap();
    let root = menus
        .load(MenuTemplate32::parse(&bytes, 8, 3).unwrap())
        .unwrap();
    let child = menus.item(root, 0, true).unwrap().submenu.unwrap();
    assert_ne!(root, child);
    assert_eq!(MenuHandle32::try_from(root.get()), Ok(root));
    assert!(!menus.popup(root).unwrap());
    assert!(menus.popup(child).unwrap());
    assert_eq!(menus.count(root), Ok(1));
    assert_eq!(menus.count(child), Ok(1));
    assert_eq!(menus.item(root, 41, false).unwrap().text, [b'X' as u16]);
    menus.set_help(child, 0x12345678).unwrap();
    assert_eq!(menus.help(child), Ok(0x12345678));
    menus.destroy(root).unwrap();
    assert!(!menus.contains(root) && !menus.contains(child));
    assert_eq!(menus.count(child), Err(E::Win32(1401)));
    assert_ne!(menus.create(false).unwrap(), root);
}

#[test]
fn remove_detaches_a_submenu_while_delete_retires_its_descendants() {
    let mut menus = MenuStore32::new(limits()).unwrap();
    let root = menus.create(false).unwrap();
    let child = menus.create(true).unwrap();
    let leaf = menus.create(true).unwrap();
    menus.insert(child, 0, popup(leaf)).unwrap();
    menus.insert(root, 0, popup(child)).unwrap();
    menus.remove(root, 0, true, false).unwrap();
    assert_eq!(menus.count(root), Ok(0));
    assert!(menus.contains(child) && menus.contains(leaf));
    menus.insert(root, 0, popup(child)).unwrap();
    menus.remove(root, 0, true, true).unwrap();
    assert_eq!(menus.count(root), Ok(0));
    assert!(!menus.contains(child) && !menus.contains(leaf));
    menus.insert(root, 0, item(9, "123456789012")).unwrap();
}

#[test]
fn command_search_and_position_search_preserve_distinct_items() {
    let mut menus = MenuStore32::new(limits()).unwrap();
    let root = menus.create(false).unwrap();
    let child = menus.create(true).unwrap();
    menus.insert(child, 0, item(9, "a")).unwrap();
    menus.insert(root, 0, popup(child)).unwrap();
    menus.insert(root, 1, item(9, "b")).unwrap();
    assert_eq!(menus.item(root, 9, false).unwrap().text, [b'a' as u16]);
    assert_eq!(menus.item(root, 1, true).unwrap().text, [b'b' as u16]);
    assert_eq!(menus.set_state(root, 9, false, 3, 3), Ok(0));
    assert_eq!(menus.item(child, 0, true).unwrap().state, 3);
    assert_eq!(menus.item(root, 1, true).unwrap().state, 0);
    menus.replace(root, 1, true, item(11, "c")).unwrap();
    assert_eq!(menus.item(root, 1, true).unwrap().id, 11);
    assert_eq!(menus.item(root, 11, false).unwrap().text, [b'c' as u16]);
}

#[test]
fn capacity_failures_preserve_menu_contents_and_publication_revision() {
    let mut menus = MenuStore32::new(MenuLimits32 {
        items: 1,
        text_units: 2,
        ..limits()
    })
    .unwrap();
    let root = menus.create(false).unwrap();
    menus.insert(root, 0, item(1, "ab")).unwrap();
    let revision = menus.revision();
    assert_eq!(menus.insert(root, 1, item(2, "c")), Err(E::Capacity));
    assert_eq!(
        menus.replace(root, 0, true, item(3, "abc")),
        Err(E::Capacity)
    );
    assert_eq!(menus.insert(root, 2, item(4, "")), Err(E::Win32(1456)));
    assert_eq!(menus.remove(root, 5, true, true), Err(E::Win32(1456)));
    assert_eq!(menus.revision(), revision);
    assert_eq!(menus.count(root), Ok(1));
    assert_eq!(menus.item(root, 0, true).unwrap().id, 1);
    assert_eq!(
        menus.item(root, 0, true).unwrap().text,
        [b'a' as u16, b'b' as u16]
    );
}

#[test]
fn cycle_and_depth_refusal_do_not_adopt_an_invalid_submenu() {
    let mut menus = MenuStore32::new(limits()).unwrap();
    let root = menus.create(false).unwrap();
    let child = menus.create(true).unwrap();
    let leaf = menus.create(true).unwrap();
    let extra = menus.create(true).unwrap();
    menus.insert(root, 0, popup(child)).unwrap();
    menus.insert(child, 0, popup(leaf)).unwrap();
    let revision = menus.revision();
    assert_eq!(menus.insert(leaf, 0, popup(root)), Err(E::Win32(87)));
    assert_eq!(menus.insert(leaf, 0, popup(extra)), Err(E::Capacity));
    assert_eq!(menus.revision(), revision);
    assert_eq!(menus.count(leaf), Ok(0));
    assert_eq!(menus.create(false), Err(E::Capacity));
}

#[test]
fn native_standard_popup_identifier_is_its_created_submenu_handle() {
    let bytes: Vec<u8> = [0u16, 0, 0x90, b'F' as u16, 0, 0x88, 37, b'X' as u16, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut menus = MenuStore32::new(limits()).unwrap();
    let root = menus
        .load(MenuTemplate32::parse(&bytes, 8, 3).unwrap())
        .unwrap();
    let popup = menus.item(root, 0, true).unwrap();
    let child = popup.submenu.unwrap();
    assert_eq!(menus.item(child, 0, true).unwrap().state, 8);
    assert_eq!(popup.id, child.get());
}

#[test]
fn native_extended_load_keeps_child_help_but_does_not_apply_header_help() {
    let bytes: Vec<u8> = [
        1u16,
        4,
        0x5678,
        0x1234,
        0x200,
        0,
        3,
        0,
        0x4321,
        0x8765,
        0x81,
        b'P' as u16,
        0,
        0,
        0x2468,
        0x1357,
        0x100,
        0,
        8,
        0,
        0xbeef,
        0xcafe,
        0x80,
        b'Z' as u16,
        0,
    ]
    .into_iter()
    .flat_map(u16::to_le_bytes)
    .collect();
    let mut menus = MenuStore32::new(limits()).unwrap();
    let root = menus
        .load(MenuTemplate32::parse(&bytes, 8, 3).unwrap())
        .unwrap();
    let popup = menus.item(root, 0, true).unwrap();
    let child = popup.submenu.unwrap();
    assert_eq!(popup.id, 0x87654321);
    assert_eq!(menus.help(child), Ok(0x13572468));
    assert_eq!(menus.help(root), Ok(0));
}

#[test]
fn native_shared_submenu_destruction_leaves_the_other_parent_link_stale() {
    let mut menus = MenuStore32::new(limits()).unwrap();
    let first = menus.create(false).unwrap();
    let second = menus.create(false).unwrap();
    let child = menus.create(true).unwrap();
    menus.insert(first, 0, popup(child)).unwrap();
    menus.insert(second, 0, popup(child)).unwrap();
    menus.destroy(first).unwrap();
    assert!(!menus.contains(child));
    assert_eq!(menus.item(second, 0, true).unwrap().submenu, Some(child));
    assert_eq!(menus.count(child), Err(E::Win32(1401)));
    menus.destroy(second).unwrap();
}
