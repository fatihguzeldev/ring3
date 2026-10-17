use super::shapes::{circle, glyph};
use crate::windows::user32::User32Error32 as E;
pub(super) fn icon(id: u16, x: i32, y: i32) -> Result<Option<u32>, E> {
    let value = match id {
        32512 => {
            if (3..=28).contains(&x) && (5..=26).contains(&y) {
                Some(if x == 3 || x == 28 || y == 5 || y == 26 {
                    0
                } else if y < 10 {
                    0x800000
                } else {
                    0xffffff
                })
            } else {
                None
            }
        }
        32513 => {
            if !circle(x, y, 14) {
                None
            } else if (x - y).abs() <= 2 || (x + y - 32).abs() <= 2 {
                Some(0xffffff)
            } else {
                Some(0x0000ff)
            }
        }
        32514 => {
            if !circle(x, y, 14) {
                None
            } else if glyph(x, y, &[14, 17, 1, 2, 4, 0, 4], 3, (9, 6)) {
                Some(0xffffff)
            } else {
                Some(0xff0000)
            }
        }
        32515 => {
            if !(3..=28).contains(&y) || (x - 16).abs() > (y - 2) / 2 {
                None
            } else if (14..=17).contains(&x) && ((11..=20).contains(&y) || (23..=25).contains(&y)) {
                Some(0)
            } else {
                Some(0x00ffff)
            }
        }
        32516 => {
            if !circle(x, y, 14) {
                None
            } else if (14..=17).contains(&x) && ((12..=25).contains(&y) || (7..=9).contains(&y)) {
                Some(0xffffff)
            } else {
                Some(0xff0000)
            }
        }
        32517 => {
            if (4..=27).contains(&x)
                && (4..=27).contains(&y)
                && x != 15
                && x != 16
                && y != 15
                && y != 16
            {
                Some(match (x < 16, y < 16) {
                    (true, true) => 0x0000ff,
                    (false, true) => 0x00aa00,
                    (true, false) => 0xff0000,
                    _ => 0x00ffff,
                })
            } else {
                None
            }
        }
        _ => return Err(E::Win32(1814)),
    };
    Ok(value)
}
