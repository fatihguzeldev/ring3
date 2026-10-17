use super::*;
fn item(text: &str) -> Item32 {
    Item32 {
        key: 0,
        revision: 0,
        cells: vec![Cell32 {
            subitem: 0,
            text: text.encode_utf16().collect(),
        }],
        state: 0,
        image: -1,
        data: 7,
        indent: 0,
        position: None,
    }
}
fn patch(item: &Item32, text: &str) -> Patch32 {
    Patch32 {
        key: item.key,
        revision: item.revision,
        subitem: 1,
        mask: 1,
        state: 0,
        state_mask: 0,
        image: 0,
        data: 0,
        indent: 0,
        text: Some(text.encode_utf16().collect()),
    }
}
#[test]
fn captured_changes_keep_row_identity_across_insertion_and_do_not_overwrite_reentrant_edits() {
    let mut list = ListView32::default();
    let row = item("first");
    list.prepare_insert(&row).unwrap();
    list.insert(0, row);
    let change = list
        .prepare_change(vec![patch(&list.items[0], "outer")], 2)
        .unwrap();
    let outer = list.publish_change(change);
    let change = list
        .prepare_change(vec![patch(&list.items[0], "inner")], 2)
        .unwrap();
    let inner = list.publish_change(change);
    assert!(list.apply(inner, false).unwrap());
    list.retire(inner);
    let inserted = item("inserted");
    list.prepare_insert(&inserted).unwrap();
    list.insert(0, inserted);
    assert!(!list.apply(outer, false).unwrap());
    assert_eq!(
        list.items[1].text(1),
        "inner".encode_utf16().collect::<Vec<_>>()
    );
    list.retire(outer);
    assert!(list.changes.is_empty());
}
#[test]
fn deleted_row_cannot_be_replaced_by_a_later_same_index_item() {
    let mut list = ListView32::default();
    let row = item("old");
    list.prepare_insert(&row).unwrap();
    list.insert(0, row);
    let change = list
        .prepare_change(vec![patch(&list.items[0], "old edit")], 1)
        .unwrap();
    let id = list.publish_change(change);
    let old = list.delete(0);
    let row = item("new");
    list.prepare_insert(&row).unwrap();
    list.insert(0, row);
    assert_ne!(list.items[0].key, old.key);
    assert!(!list.apply(id, false).unwrap());
    assert!(list.items[0].text(1).is_empty());
}
#[test]
fn report_and_list_geometry_use_the_same_rows_with_header_and_scroll_offsets() {
    let mut list = ListView32::default();
    for _ in 0..4 {
        let row = item("row");
        list.prepare_insert(&row).unwrap();
        list.insert(list.items.len(), row);
    }
    list.top = 1;
    assert_eq!(
        list.rect(1, 1, 200, 100, 20, 18),
        Rect32 {
            left: 0,
            top: 18,
            right: 200,
            bottom: 38
        }
    );
    assert_eq!(
        list.rect(3, 3, 240, 40, 20, 0),
        Rect32 {
            left: 120,
            top: 0,
            right: 240,
            bottom: 20
        }
    );
}

#[test]
fn accepted_text_change_rechecks_aggregate_budget_after_an_unrelated_reentrant_insertion() {
    let mut list = ListView32::default();
    let first = item("old");
    list.prepare_insert(&first).unwrap();
    list.insert(0, first);
    let large = "x".repeat(MAX_TEXT);
    for _ in 0..7 {
        let row = item(&large);
        list.prepare_insert(&row).unwrap();
        list.insert(list.items.len(), row);
    }
    let change = list
        .prepare_change(vec![patch(&list.items[0], &"y".repeat(30000))], 2)
        .unwrap();
    let id = list.publish_change(change);
    let unrelated = item(&large);
    list.prepare_insert(&unrelated).unwrap();
    list.insert(list.items.len(), unrelated);
    let revision = list.items[0].revision;
    let count = list.items.len();
    assert_eq!(list.apply(id, false), Err(E::Capacity));
    assert_eq!(list.items[0].revision, revision);
    assert_eq!(list.items.len(), count);
    assert!(list.items[0].text(1).is_empty());
    assert_eq!(list.change(id).unwrap().cursor, 0);
    assert_eq!(
        list.change(id).unwrap().patches[0]
            .text
            .as_ref()
            .unwrap()
            .len(),
        30000
    );
}
