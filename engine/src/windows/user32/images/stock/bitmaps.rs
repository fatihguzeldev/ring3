use crate::windows::user32::{SystemColors32, User32Error32 as E};
pub(super) fn bitmap(
    id: u16,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    colors: &SystemColors32,
) -> Result<Option<u32>, E> {
    if id == 32759 {
        let cell_x = x % 13;
        let cell_y = y % 13;
        let column = x / 13;
        let row = y / 13;
        let border = (1..=11).contains(&cell_x)
            && (1..=11).contains(&cell_y)
            && (cell_x == 1 || cell_x == 11 || cell_y == 1 || cell_y == 11);
        let check = row == 1 && (cell_x - cell_y - 1).abs() <= 1 && (5..=9).contains(&cell_x)
            || row == 1 && (cell_x + cell_y - 12).abs() <= 1 && (3..=5).contains(&cell_x);
        let mixed = row == 2 && (4..=8).contains(&cell_x) && (4..=8).contains(&cell_y);
        return Ok(Some(if border || check || mixed {
            if column == 2 { 0x808080 } else { 0 }
        } else if column == 1 {
            0xc0c0c0
        } else {
            0xffffff
        }));
    }
    let x = x * 16 / width;
    let y = y * 16 / height;
    let mark = match id {
        32754 | 32767 => {
            (4..=11).contains(&x)
                && (4..=11).contains(&y)
                && ((x - y).abs() <= 1 || (x + y - 15).abs() <= 1)
        }
        32753 | 32743 | 32737 | 32765 => (5..=10).contains(&y) && (x - 8).abs() <= y - 5,
        32752 | 32742 | 32736 | 32764 | 32738 => (5..=10).contains(&y) && (x - 8).abs() <= 10 - y,
        32751 | 32741 | 32735 | 32763 | 32739 => (5..=10).contains(&x) && (y - 8).abs() <= 10 - x,
        32750 | 32740 | 32734 | 32762 => (5..=10).contains(&x) && (y - 8).abs() <= x - 5,
        32749 | 32746 | 32757 => (4..=11).contains(&x) && (10..=12).contains(&y),
        32748 | 32745 | 32756 => {
            (3..=12).contains(&x)
                && (3..=12).contains(&y)
                && (x == 3 || x == 12 || y <= 5 || y == 12)
        }
        32747 | 32744 | 32755 => {
            (5..=13).contains(&x)
                && (2..=10).contains(&y)
                && (x == 5 || x == 13 || y <= 4 || y == 10)
                || (2..=10).contains(&x)
                    && (5..=13).contains(&y)
                    && (x == 2 || x == 10 || y <= 7 || y == 13)
        }
        32760 => {
            (3..=6).contains(&x) && (x - y + 5).abs() <= 1
                || (6..=12).contains(&x) && (x + y - 17).abs() <= 1
        }
        32766 => x >= 4 && y >= 4 && x + y >= 16 && (x + y) % 3 == 0,
        32758 => (2..=13).contains(&x) && (2..=13).contains(&y) && (x == 2 || y == 2),
        32761 => {
            (3..=12).contains(&x)
                && (3..=12).contains(&y)
                && (x == 3 || x == 12 || y == 3 || y == 12)
        }
        _ => return Err(E::Win32(1814)),
    };
    Ok(Some(if mark {
        colors.get(18).unwrap()
    } else {
        colors.get(15).unwrap()
    }))
}
