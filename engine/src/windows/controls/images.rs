use crate::windows::user32::images::ImageKind32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlImage32 {
    pub kind: ImageKind32,
    pub handle: u32,
}

pub(crate) fn static_image_kind(style: u32) -> Option<ImageKind32> {
    match style & 31 {
        3 => Some(ImageKind32::Icon),
        14 => Some(ImageKind32::Bitmap),
        _ => None,
    }
}
