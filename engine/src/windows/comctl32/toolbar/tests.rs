use super::*;
fn button(command: u32, style: u8) -> ToolbarButton32 {
    ToolbarButton32 {
        key: 0,
        bitmap: -2,
        command,
        state: 4,
        style,
        data: 0,
        string: u32::MAX,
        text: vec![65],
    }
}
#[test]
fn inserted_buttons_keep_identity_and_group_checks_do_not_cross_separators() {
    let mut bar = Toolbar32::default();
    bar.insert(0, vec![button(1, 6), button(2, 6), button(3, 0)])
        .unwrap();
    let key = bar.buttons[1].key;
    bar.check_button(0, true);
    bar.check_button(1, true);
    assert_eq!(bar.buttons[0].state & 1, 0);
    assert_eq!(bar.buttons[1].state & 1, 1);
    bar.insert(0, vec![button(4, 0)]).unwrap();
    assert_eq!(bar.buttons[2].key, key);
    bar.pressed = Some(key);
    bar.remove(2);
    assert_eq!(bar.pressed, None);
    bar.insert(2, vec![button(5, 0)]).unwrap();
    assert_ne!(bar.buttons[2].key, key);
}
#[test]
fn hidden_geometry_and_rejected_bitmap_preserve_existing_buttons_and_issuer() {
    let mut bar = Toolbar32::default();
    bar.insert(0, vec![button(1, 0), button(2, 0)]).unwrap();
    bar.buttons[0].state |= 8;
    assert_eq!(bar.rect(1).unwrap().left, 0);
    assert_eq!(bar.hit(Point32 { x: 1, y: 1 }), Some(1));
    let next = bar.next;
    let mut bad = button(3, 0);
    bad.bitmap = 0;
    assert_eq!(bar.insert(0, vec![bad]), Err(E::Win32(50)));
    assert_eq!(bar.buttons.len(), 2);
    assert_eq!(bar.next, next);
}
