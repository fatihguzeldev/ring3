use super::*;

#[test]
fn native_progress_rows_preserve_signed_ranges_previous_values_clamping_wrapping_and_colors() {
    let mut progress = Progress32::default();
    assert_eq!(
        (
            progress.minimum,
            progress.maximum,
            progress.position,
            progress.step
        ),
        (0, 100, 0, 10)
    );
    let cases = [
        (1, 10, 20, 6_553_600u32, 10, 20, 10),
        (2, -5, 0, 10, 10, 20, 10),
        (2, 17, 0, 10, 10, 20, 17),
        (2, 30, 0, 17, 10, 20, 20),
        (3, 7, 0, 10, 10, 20, 20),
        (4, 0, 0, 20, 10, 20, 17),
        (5, -40, 0, 17, 10, 20, 10),
        (3, -3, 0, 7, 10, 20, 10),
        (4, 0, 0, 10, 10, 20, 17),
        (1, -200_000, 300_000, 1_310_730, -200_000, 300_000, 17),
        (2, 123_456, 0, 17, -200_000, 300_000, 123_456),
        (1, 50, 10, (-1_813_974_336i32) as u32, 50, 10, 10),
        (1, 7, 7, 655_410, 7, 7, 7),
        (1, i32::MIN, i32::MAX, 458_759, i32::MIN, i32::MAX, 7),
        (3, i32::MAX, 0, (-3i32) as u32, i32::MIN, i32::MAX, 7),
        (2, i32::MAX, 0, 7, i32::MIN, i32::MAX, i32::MAX),
        (4, 0, 0, i32::MAX as u32, i32::MIN, i32::MAX, -2),
        (
            5,
            i32::MAX,
            0,
            (-2i32) as u32,
            i32::MIN,
            i32::MAX,
            2_147_483_645,
        ),
    ];
    for (operation, first, second, old, low, high, position) in cases {
        let result = match operation {
            1 => progress.set_range(first, second),
            2 => progress.set_position(first),
            3 => progress.set_step(first),
            4 => progress.advance(),
            5 => progress.delta(first),
            _ => unreachable!(),
        };
        assert_eq!(
            (
                result,
                progress.minimum,
                progress.maximum,
                progress.position
            ),
            (old, low, high, position)
        );
    }
    assert_eq!(
        (progress.bar, progress.background),
        (0xff00_0000, 0xff00_0000)
    );
}

#[test]
fn progress_zero_or_inverted_ranges_and_extreme_steps_never_divide_or_overflow() {
    let mut progress = Progress32::default();
    progress.set_range(7, 7);
    progress.set_step(i32::MIN);
    assert_eq!(progress.advance(), 7);
    assert_eq!(progress.position, 7);
    progress.set_range(50, 10);
    assert_eq!(progress.position, 10);
    assert_eq!(progress.delta(i32::MIN), 10);
    assert_eq!(progress.position, 10);
}
