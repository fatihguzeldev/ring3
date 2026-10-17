use super::*;
#[test]
fn integer_extreme_ranges_and_geometry_stay_bounded() {
    let mut bar = Trackbar32::default();
    bar.set_range(i32::MIN, i32::MAX).unwrap();
    let rect = bar.channel(96, 24, false);
    assert_eq!(bar.value_at(rect.left, rect, false), i32::MIN);
    assert_eq!(bar.value_at(rect.right, rect, false), i32::MAX);
    assert_eq!(bar.coordinate(i32::MAX, rect, false), rect.right);
    assert!(bar.set_frequency(1).is_err());
    assert_eq!(bar.frequency, None);
    assert_eq!((bar.minimum, bar.maximum), (i32::MIN, i32::MAX));
}
#[test]
fn automatic_and_manual_ticks_preserve_endpoint_and_range_contracts() {
    let mut bar = Trackbar32::default();
    bar.set_frequency(25).unwrap();
    assert_eq!(bar.tick_count(), 3);
    assert_eq!(bar.tick(0), Some(25));
    assert_eq!(bar.tick(2), Some(75));
    assert_eq!(bar.tick(3), None);
    bar.set_frequency(0).unwrap();
    assert!(bar.add_tick(40).unwrap());
    assert!(bar.add_tick(20).unwrap());
    assert!(!bar.add_tick(100).unwrap());
    assert_eq!(bar.ticks, [20, 40]);
    bar.set_range(30, 50).unwrap();
    assert_eq!(bar.ticks, [40]);
}
