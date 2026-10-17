use super::shapes::{arrow, circle, glyph, outlined};
use crate::windows::user32::User32Error32 as E;
pub(super) fn cursor(id: u16, x: i32, y: i32) -> Result<Option<u32>, E> {
    let value = match id {
        32512 | 32641 => outlined(x, y, arrow),
        32513 => outlined(x, y, |x, y| {
            (14..=17).contains(&x) && (4..=28).contains(&y)
                || (10..=21).contains(&x) && [4, 5, 27, 28].contains(&y)
        }),
        32514 => outlined(x, y, |x, y| {
            (7..=24).contains(&x) && [4, 5, 27, 28].contains(&y)
                || (6..=26).contains(&y) && ((x - 16).abs() - (y - 16).abs()).abs() <= 1
        }),
        32515 => outlined(x, y, |x, y| {
            (14..=17).contains(&x) && (4..=28).contains(&y)
                || (14..=17).contains(&y) && (4..=28).contains(&x)
        }),
        32516 => outlined(x, y, |x, y| {
            (13..=18).contains(&x) && (13..=29).contains(&y)
                || (2..=15).contains(&y) && (x - 16).abs() <= y
        }),
        32640 | 32642..=32646 => {
            let diagonal = if id == 32642 {
                (x - y).abs() <= 1
            } else if id == 32643 {
                (x + y - 31).abs() <= 1
            } else {
                false
            };
            let horizontal = matches!(id, 32640 | 32644 | 32646) && (y - 16).abs() <= 1;
            let vertical = matches!(id, 32640 | 32645 | 32646) && (x - 16).abs() <= 1;
            ((4..=27).contains(&x) && (4..=27).contains(&y) && (diagonal || horizontal || vertical))
                .then_some(0)
        }
        32648 => {
            if circle(x, y, 13) && (!circle(x, y, 10) || (x - y).abs() <= 2) {
                Some(0x0000ff)
            } else {
                None
            }
        }
        32649 => outlined(x, y, |x, y| {
            (10..=13).contains(&x) && (4..=26).contains(&y)
                || (9..=23).contains(&x) && (15..=27).contains(&y)
                || (5..=10).contains(&x) && (15..=20).contains(&y)
        }),
        32650 => {
            let wait = cursor(32514, x - 9, y + 4)?;
            if (18..=30).contains(&x) && y < 22 {
                wait
            } else {
                outlined(x, y, arrow)
            }
        }
        32651 => {
            if glyph(x, y, &[14, 17, 1, 2, 4, 0, 4], 2, (20, 2)) {
                Some(0)
            } else {
                outlined(x, y, arrow)
            }
        }
        _ => return Err(E::Win32(1814)),
    };
    Ok(value)
}
