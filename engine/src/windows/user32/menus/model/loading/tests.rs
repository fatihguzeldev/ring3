use super::*;

fn template() -> Template32 {
    let bytes: Vec<u8> = [0u16, 0, 0x90, 70, 0, 0x80, 37, 88, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    Template32::parse(&bytes, 8, 3).unwrap()
}

#[test]
fn prepared_resource_menus_publish_once_and_failed_preparation_does_not_issue_handles() {
    let mut store = MenuStore32::new(Default::default()).unwrap();
    let first = store.prepare_load(template()).unwrap();
    let second = store.prepare_load(template()).unwrap();
    assert_eq!(first.root(), second.root());
    assert_eq!(store.revision(), 0);
    assert!(!store.contains(first.root()));
    let root = store.commit_load(first);
    assert_eq!(store.revision(), 1);
    assert_eq!(store.check_load(&second), Err(E::InvalidRequest));
    let child = store.item(root, 0, true).unwrap().submenu.unwrap();
    assert_eq!(store.item(child, 0, true).unwrap().id, 37);
    assert_eq!(store.item(child, 0, true).unwrap().text, [88]);
    let next = store.prepare_load(template()).unwrap();
    assert_ne!(root, next.root());
    assert!(!store.contains(next.root()));
}

#[test]
fn resource_menu_capacity_and_revision_exhaustion_leave_existing_tree_intact() {
    let mut store = MenuStore32::new(crate::windows::user32::menus::MenuLimits32 {
        menus: 2,
        items: 2,
        text_units: 2,
        depth: 3,
    })
    .unwrap();
    let root = store.load(template()).unwrap();
    let revision = store.revision();
    assert!(matches!(store.prepare_load(template()), Err(E::Capacity)));
    assert_eq!(store.revision(), revision);
    assert_eq!(store.count(root), Ok(1));
    let mut store = MenuStore32::new(Default::default()).unwrap();
    let root = store.load(template()).unwrap();
    store.revision = u64::MAX;
    assert!(matches!(store.prepare_load(template()), Err(E::Capacity)));
    assert_eq!(store.revision(), u64::MAX);
    assert_eq!(store.count(root), Ok(1));
}
