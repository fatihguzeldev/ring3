use super::*;
fn item(width: i32, unit: u16) -> HeaderItem32 {
    HeaderItem32 {
        width,
        text: vec![unit],
        format: 0x4000,
        ..Default::default()
    }
}
#[test]
fn order_and_identity_survive_insertion_deletion_and_resize() {
    let mut header = Header32::default();
    for (index, width) in [20, 30, 40].into_iter().enumerate() {
        let next = item(width, 65 + index as u16);
        header.prepare_insert(&next).unwrap();
        header.insert(index, index, next);
    }
    header.check_order(&[2, 0, 1]).unwrap();
    header.order = vec![2, 0, 1];
    assert_eq!(
        header.rect(0, 18).unwrap(),
        Rect32 {
            left: 40,
            top: 0,
            right: 60,
            bottom: 18
        }
    );
    let first = header.items[0].key;
    assert!(header.check_order(&[0, 0, 2]).is_err());
    assert_eq!(header.order, [2, 0, 1]);
    header.delete(0);
    let next = item(15, 90);
    header.prepare_insert(&next).unwrap();
    header.insert(0, 0, next);
    assert_ne!(header.items[0].key, first);
    assert_eq!(header.order, [0, 2, 1]);
    header.move_item(1, 0);
    assert_eq!(header.order, [1, 0, 2]);
}
#[test]
fn malformed_and_exhausted_items_leave_original_geometry_current() {
    let mut header = Header32::default();
    let next = item(i32::MAX, 65);
    header.prepare_insert(&next).unwrap();
    header.insert(0, 0, next);
    assert!(header.prepare_insert(&item(1, 66)).is_err());
    assert!(header.check_item(Some(0), &item(-1, 66)).is_err());
    assert_eq!(header.items[0].width, i32::MAX);
    assert_eq!(header.order, [0]);
    let mut next = item(10, 66);
    next.text.resize(MAX_TEXT + 1, 65);
    assert!(header.check_item(Some(0), &next).is_err());
    assert_eq!(header.items[0].text, [65]);
}
#[test]
fn hit_test_distinguishes_divider_item_and_outside() {
    let mut header = Header32::default();
    for (index, width) in [20, 0, 30].into_iter().enumerate() {
        let next = item(width, 65);
        header.prepare_insert(&next).unwrap();
        header.insert(index, index, next);
    }
    assert_eq!(header.hit(Point32 { x: 19, y: 3 }, 80, 18), (0, 4));
    assert_eq!(header.hit(Point32 { x: 8, y: 3 }, 80, 18), (0, 2));
    assert_eq!(header.hit(Point32 { x: 70, y: 3 }, 80, 18), (-1, 1));
    assert_eq!(header.hit(Point32 { x: -1, y: 19 }, 80, 18), (-1, 0xa00));
}
