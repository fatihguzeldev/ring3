mod bitmaps;
mod calls;
mod colors;
mod context;
pub(in crate::process) mod controls;
mod fonts;
mod memory;
mod objects;
mod paint;
mod palette;
mod raster;
mod regions;
mod runtime;
mod state;
mod storage;
mod types;
pub(in crate::process::windows) mod windows;

pub(in crate::process) use context::GdiRuntime32;
pub use types::{GdiConfig32, GdiFrame32};

#[cfg(test)]
mod tests;
