use super::{DispatchError, gdi};

const LARGE_ICON: u32 = 32;
const SMALL_ICON: u32 = 16;
const SCROLL_WIDTH: u32 = 16;
const SCROLL_HEIGHT: u32 = 16;

pub(super) fn get(index: u32) -> Result<u32, DispatchError> {
    match index {
        0 => Ok(gdi::SCREEN_WIDTH),
        1 => Ok(gdi::SCREEN_HEIGHT),
        2 | 3 => Ok(SCROLL_WIDTH),
        9 | 10 | 20 | 21 => Ok(SCROLL_HEIGHT),
        11 | 12 => Ok(LARGE_ICON),
        49 | 50 => Ok(SMALL_ICON),
        _ => Err(DispatchError::Unsupported),
    }
}
