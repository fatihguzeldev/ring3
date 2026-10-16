mod info;
mod objects;
mod section;
mod transfers;

pub(in crate::process::windows::gdi) use info::{color_to_quad, dib_layout, quad_to_color};
pub(in crate::process::windows::gdi) use objects::bounds;
