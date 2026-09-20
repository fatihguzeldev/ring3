use super::{DispatchError, gdi};

const LARGE_ICON: u32 = 32;
const SMALL_ICON: u32 = 16;
const SCROLL_WIDTH: u32 = 16;
const SCROLL_HEIGHT: u32 = 16;

fn metrics(index: u32) -> Result<u32, DispatchError> {
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

#[derive(Clone, Copy)]
pub(super) enum Call {
    Metrics,
    Color,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x9c => Some(Self::Metrics),
            0xac => Some(Self::Color),
            _ => None,
        }
    }

    pub(super) fn dispatch(self, index: u32) -> Result<u32, DispatchError> {
        match self {
            Self::Metrics => metrics(index),
            Self::Color => color(index),
        }
    }
}

pub(super) fn color(index: u32) -> Result<u32, DispatchError> {
    match index {
        0 | 4 | 10 | 11 | 15 | 19 | 22 | 30 => Ok(0x00c8_d0d4),
        1 => Ok(0x00a5_6e3a),
        2 | 13 | 29 => Ok(0x006a_240a),
        3 | 12 | 16 | 17 => Ok(0x0080_8080),
        5 | 9 | 14 | 20 => Ok(0x00ff_ffff),
        21 => Ok(0x0040_4040),
        24 => Ok(0x00e1_ffff),
        26 => Ok(0x00c8_0000),
        27 => Ok(0x00f0_caa6),
        28 => Ok(0x00c0_c0c0),
        25 => Err(DispatchError::Unsupported),
        // black elements and out-of-range indices both return zero.
        _ => Ok(0),
    }
}
