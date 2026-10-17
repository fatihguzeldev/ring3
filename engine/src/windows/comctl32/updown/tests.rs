use super::*;
#[test]
fn reversed_ranges_wrap_and_extreme_spans_do_not_overflow() {
    let mut spin = UpDown32::default();
    assert_eq!(spin.delta(100, 1, true), 0);
    spin.set_range(100, 0);
    assert_eq!(spin.delta(50, 1, false), 49);
    assert_eq!(spin.delta(0, 1, true), 100);
    spin.set_range(i32::MIN, i32::MAX);
    assert_eq!(spin.delta(i32::MAX, 1, true), i32::MIN);
}
#[test]
fn buddy_format_and_acceleration_use_actual_numeric_state() {
    let mut spin = UpDown32::default();
    spin.set_range(-10000, 10000);
    assert_eq!(
        spin.format(-1234, true),
        "-1,234".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        spin.parse(&"-1,234".encode_utf16().collect::<Vec<_>>()),
        Some(-1234)
    );
    spin.base = 16;
    assert_eq!(
        spin.format(31, true),
        "001F".encode_utf16().collect::<Vec<_>>()
    );
    spin.acceleration = vec![(0, 1), (2, 10)];
    assert_eq!(spin.step(1), 1);
    assert_eq!(spin.step(3), 10);
    assert!(UpDown32::check_acceleration(&[(2, 1), (1, 4)]).is_err());
}
