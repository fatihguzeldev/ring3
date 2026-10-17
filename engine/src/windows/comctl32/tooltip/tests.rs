use super::*;
fn tool() -> Tool32 {
    Tool32 {
        key: 0,
        revision: 1,
        callback: false,
        callback_owner: 0,
        tab_owner: 0,
        tab_key: 0,
        window: 7,
        id: 8,
        flags: 0xa0,
        rect: Rect32::default(),
        data: 9,
        text: vec![65],
    }
}
#[test]
fn deleted_tool_identity_is_not_reused_by_the_same_public_key() {
    let mut tips = Tooltip32::default();
    let first = tool();
    tips.prepare(&first, None).unwrap();
    tips.insert(first);
    tips.active = Some(tips.tools[0].key);
    let old = tips.active;
    tips.remove(0);
    assert_eq!(tips.active, None);
    let next = tool();
    tips.prepare(&next, None).unwrap();
    tips.insert(next);
    assert_ne!(Some(tips.tools[0].key), old);
    assert_eq!(tips.current(), None);
}
#[test]
fn over_budget_text_does_not_replace_the_live_tool() {
    let mut tips = Tooltip32::default();
    let first = tool();
    tips.prepare(&first, None).unwrap();
    tips.insert(first);
    let mut next = tool();
    next.text = vec![66; MAX_TEXT + 1];
    assert!(tips.prepare(&next, Some(0)).is_err());
    assert_eq!(tips.tools[0].text, [65]);
}
