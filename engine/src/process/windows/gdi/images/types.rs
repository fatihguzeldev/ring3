use crate::windows::user32::Point32;

#[derive(Clone, Copy)]
pub(in crate::process) struct GdiDibSource32<'a> {
    pub info: &'a [u8],
    pub pixels: Option<&'a [u8]>,
}
#[derive(Clone, Copy, Default)]
pub(in crate::process) struct ResourceBitmapOptions32 {
    pub width: u32,
    pub height: u32,
    pub monochrome: bool,
    pub dib_section: bool,
    pub transparent: bool,
    pub map3d: bool,
    pub vga: bool,
}
pub(in crate::process) use crate::windows::gdi::GdiImageKind32;
#[derive(Clone, Copy)]
pub(in crate::process) struct GdiImageRequest32<'a> {
    pub kind: GdiImageKind32,
    pub dib: GdiDibSource32<'a>,
    pub hotspot: Point32,
    pub width: u32,
    pub height: u32,
    pub monochrome: bool,
    pub transparent: bool,
    pub map3d: bool,
    pub vga: bool,
}

#[derive(Clone, Copy)]
pub(in crate::process) struct GdiBitmapInfo32 {
    pub width: u32,
    pub height: u32,
    pub monochrome: bool,
    pub dib_section: bool,
}
#[derive(Clone, Copy)]
pub(in crate::process) struct GdiBitmapView32<'a> {
    pub layout: crate::windows::gdi::BitmapLayout32,
    pub palette: &'a [u32],
    pub pixels: &'a [u8],
}
#[derive(Clone, Copy)]
pub(in crate::process) struct GdiImageInfo32<'a> {
    pub kind: GdiImageKind32,
    pub width: u32,
    pub height: u32,
    pub monochrome: bool,
    pub hotspot: Point32,
    pub alpha: bool,
    pub mask: GdiBitmapView32<'a>,
    pub color: Option<GdiBitmapView32<'a>>,
}
#[derive(Clone, Copy)]
pub(in crate::process) struct GdiImageDraw32 {
    pub handle: u32,
    pub position: Point32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
    pub background: u32,
}
