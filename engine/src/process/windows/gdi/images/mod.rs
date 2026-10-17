mod bitmaps;
mod copying;
mod decoding;
mod ownership;
mod planes;
mod preparation;
mod types;

pub(in crate::process::windows::gdi) use ownership::ImageOwner32;
pub(in crate::process) use types::{
    GdiDibSource32, GdiImageDraw32, GdiImageKind32, GdiImageRequest32, ResourceBitmapOptions32,
};
