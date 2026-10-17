pub(in crate::process::windows::gdi) mod canvas;
mod drawing;
mod pixels;

pub(super) use drawing::{BlitImage32, BlitRequest32};

mod stretch;

mod msimg;

pub(in crate::process::windows::gdi) use drawing::binary_rop;

mod decoration;
