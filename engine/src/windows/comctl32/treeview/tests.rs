use super::*;
fn node(id: u32, parent: u32, text: &str) -> Item32 {
    Item32 {
        id,
        parent,
        revision: 1,
        text: text.encode_utf16().collect(),
        state: 0,
        image: 0,
        selected_image: 0,
        children: 0,
        data: id * 100,
    }
}
fn insert(tree: &mut TreeView32, item: Item32, after: u32) {
    let i = tree.prepare_insert(item.parent, after, &item).unwrap();
    tree.insert(i, item);
}
#[test]
fn hierarchy_siblings_and_expansion_use_stable_items() {
    let mut tree = TreeView32::default();
    insert(&mut tree, node(1, 0, "root"), LAST);
    insert(&mut tree, node(2, 1, "child"), LAST);
    insert(&mut tree, node(3, 0, "other"), LAST);
    insert(&mut tree, node(4, 1, "first"), FIRST);
    assert_eq!(
        tree.items.iter().map(|item| item.id).collect::<Vec<_>>(),
        [1, 4, 2, 3]
    );
    assert_eq!(tree.visible_ids().unwrap(), [1, 3]);
    tree.item_mut(1).unwrap().state = 0x20;
    assert_eq!(tree.visible_ids().unwrap(), [1, 4, 2, 3]);
    assert_eq!(tree.depth(2), 1);
    assert_eq!(tree.first_child(1), 4);
    tree.remove_leaf(4);
    assert_eq!(tree.first_child(1), 2);
    assert_eq!(tree.deletion_leaf(1), 2);
    tree.remove_leaf(2);
    assert_eq!(tree.deletion_leaf(1), 1);
    assert_eq!(tree.items[1].data, 300);
}
#[test]
fn rejected_insert_has_no_published_node_and_text_budget_is_aggregate() {
    let mut tree = TreeView32::default();
    insert(&mut tree, node(1, 0, "root"), LAST);
    assert_eq!(
        tree.prepare_insert(999, LAST, &node(2, 999, "bad")),
        Err(E::Win32(87))
    );
    assert_eq!(tree.items.len(), 1);
    assert_eq!(tree.prepare_insert(0, 1, &node(2, 0, "good")), Ok(1));
    let huge = vec![65u16; MAX_TEXT + 1];
    assert_eq!(tree.check_text(None, &huge), Err(E::Win32(87)));
    assert_eq!(
        tree.items[0].text,
        "root".encode_utf16().collect::<Vec<_>>()
    );
}
