mod bitmaps;
mod calls;
mod colors;
mod context;
pub(in crate::process) mod controls;
mod fonts;
mod images;
mod memory;
mod objects;
mod paint;
mod palette;
mod patterns;
mod raster;
mod regions;
mod runtime;
mod state;
mod storage;
mod types;
pub(in crate::process::windows) mod windows;

pub(in crate::process) use context::GdiRuntime32;
pub(in crate::process) use images::{
    GdiDibSource32, GdiImageDraw32, GdiImageKind32, GdiImageRequest32, ResourceBitmapOptions32,
};
pub(in crate::process) use raster::canvas::{CanvasText32, GdiCanvas32};
pub use types::{GdiConfig32, GdiFrame32};

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::process::windows) use tests::captured::call_id as captured_gdi_call_for_test;
