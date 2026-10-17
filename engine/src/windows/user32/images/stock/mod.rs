mod bitmaps;
mod cursors;
mod icons;
mod shapes;
use super::{ImageExtent32, ImageKind32};
use crate::windows::user32::{SystemColors32, User32Error32 as E};

pub(crate) struct StockImage32 {
    pub info: Vec<u8>,
    pub extent: ImageExtent32,
    pub hotspot: (u16, u16),
}
impl StockImage32 {
    pub(crate) fn load(kind: ImageKind32, id: u16, colors: &SystemColors32) -> Result<Self, E> {
        let (width, height) = if kind == ImageKind32::Bitmap {
            match id {
                32738 | 32758 => (13usize, 13usize),
                32739 | 32760 => (15, 15),
                32744 | 32746 => (17, 15),
                32745 | 32748 => (20, 18),
                32747 | 32749 => (17, 19),
                32754 => (42, 23),
                32759 => (52, 39),
                32734..=32767 => (17, 17),
                _ => return Err(E::Win32(1814)),
            }
        } else {
            (32, 32)
        };
        let monochrome = kind == ImageKind32::Cursor && matches!(id, 32513 | 32515)
            || kind == ImageKind32::Bitmap && matches!(id, 32739 | 32760);
        let hotspot = if kind == ImageKind32::Cursor {
            match id {
                32512 | 32641 | 32651 => (0, 0),
                32513 => (8, 9),
                32514 | 32515 | 32640 => (16, 16),
                32516 => (4, 0),
                32642 | 32643 | 32648 => (8, 8),
                32644 => (11, 4),
                32645 => (4, 11),
                32646 => (11, 11),
                32649 => (6, 0),
                32650 => (0, 8),
                _ => return Err(E::Win32(1814)),
            }
        } else {
            (0, 0)
        };
        let stride = if monochrome {
            width.div_ceil(32) * 4
        } else {
            width * 4
        };
        let mask_stride = width.div_ceil(32) * 4;
        let mask_bytes = if kind == ImageKind32::Bitmap {
            0
        } else {
            mask_stride * height
        };
        let pixel_start = 40 + if monochrome { 8 } else { 0 };
        let mut info = Vec::new();
        info.try_reserve_exact(pixel_start + stride * height + mask_bytes)
            .map_err(|_| E::Allocation)?;
        info.resize(pixel_start + stride * height + mask_bytes, 0);
        if monochrome {
            info[44..48].copy_from_slice(&[255, 255, 255, 0]);
        }
        for (at, value) in [
            (0, 40),
            (4, width as u32),
            (
                8,
                height as u32 * if kind == ImageKind32::Bitmap { 1 } else { 2 },
            ),
            (12, 1 | if monochrome { 1 << 16 } else { 32 << 16 }),
        ] {
            info[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for y in 0..height {
            for x in 0..width {
                let color = match kind {
                    ImageKind32::Bitmap => bitmaps::bitmap(
                        id,
                        x as i32,
                        y as i32,
                        width as i32,
                        height as i32,
                        colors,
                    )?,
                    ImageKind32::Icon => icons::icon(id, x as i32, y as i32)?,
                    ImageKind32::Cursor => cursors::cursor(id, x as i32, y as i32)?,
                };
                let row = height - 1 - y;
                if let Some(color) = color {
                    if monochrome {
                        if color == 0xffffff {
                            info[pixel_start + row * stride + x / 8] |= 0x80 >> (x % 8);
                        }
                    } else {
                        let at = pixel_start + row * stride + x * 4;
                        info[at..at + 4].copy_from_slice(&[
                            (color >> 16) as u8,
                            (color >> 8) as u8,
                            color as u8,
                            0,
                        ]);
                    }
                } else if mask_bytes != 0 {
                    info[pixel_start + stride * height + row * mask_stride + x / 8] |=
                        0x80 >> (x % 8);
                }
            }
        }
        Ok(Self {
            info,
            extent: ImageExtent32 {
                width: width as u32,
                height: height as u32,
            },
            hotspot,
        })
    }
}
