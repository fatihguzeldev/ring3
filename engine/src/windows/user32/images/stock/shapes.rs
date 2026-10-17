pub(super) fn outlined(x: i32, y: i32, shape: impl Fn(i32, i32) -> bool) -> Option<u32> {
    if !shape(x, y) {
        return None;
    }
    Some(
        if [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
            .into_iter()
            .all(|(x, y)| shape(x, y))
        {
            0xffffff
        } else {
            0
        },
    )
}
pub(super) fn arrow(x: i32, y: i32) -> bool {
    (1..=23).contains(&y) && x >= 2 && x <= 2 + y / 2
        || (14..=28).contains(&y) && (x - (y / 2 - 1)).abs() <= 2
}
pub(super) fn circle(x: i32, y: i32, r: i32) -> bool {
    (x - 16) * (x - 16) + (y - 16) * (y - 16) <= r * r
}
pub(super) fn glyph(x: i32, y: i32, lines: &[u8; 7], scale: i32, origin: (i32, i32)) -> bool {
    let x = x - origin.0;
    let y = y - origin.1;
    x >= 0
        && y >= 0
        && x < 5 * scale
        && y < 7 * scale
        && lines[(y / scale) as usize] & (1 << (4 - x / scale)) != 0
}
