use super::*;
fn item(value: u16) -> TabItem32 {
    TabItem32 {
        text: vec![value],
        data: vec![0; 4],
        ..Default::default()
    }
}
#[test]
fn insertion_and_deletion_keep_existing_selection_and_owned_payloads() {
    let mut tab = Tab32::default();
    let first = item(65);
    tab.prepare_insert(&first).unwrap();
    tab.insert(7, first);
    assert_eq!((tab.selected, tab.focused), (0, 0));
    let second = item(66);
    tab.prepare_insert(&second).unwrap();
    tab.insert(0, second);
    assert_eq!((tab.selected, tab.focused), (1, 1));
    assert_eq!(tab.items[1].text, [65]);
    assert_eq!(tab.select(0), 1);
    assert!(tab.delete(0));
    assert_eq!((tab.selected, tab.focused), (-1, -1));
    assert_eq!(tab.items[0].text, [65]);
}
#[test]
fn payload_and_text_budget_refusals_do_not_replace_live_items() {
    let mut tab = Tab32::default();
    let first = item(65);
    tab.prepare_insert(&first).unwrap();
    tab.insert(0, first);
    let mut next = item(66);
    next.data.pop();
    assert!(tab.check_item(Some(0), &next).is_err());
    next.data.push(0);
    next.text = vec![67; MAX_TEXT + 1];
    assert!(tab.check_item(Some(0), &next).is_err());
    assert_eq!(tab.items[0].text, [65]);
}
