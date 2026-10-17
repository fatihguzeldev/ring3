use super::ControlVisual32;
use crate::{
    memory::AddressSpace,
    process::windows::gdi::{GdiCanvas32, GdiImageDraw32, GdiRuntime32},
    windows::{
        gdi::GdiError32,
        user32::{Point32, Rect32, images::ImageKind32},
    },
};

pub(super) fn paint(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
) -> Result<(), GdiError32> {
    let Some(image) = visual.control.image else {
        return Ok(());
    };
    let (width, height) = if image.kind == ImageKind32::Bitmap {
        if visual.style & 31 != 14 {
            return Err(GdiError32::InvalidRequest);
        }
        let info = gdi.bitmap_info(memory, image.handle)?;
        (info.width, info.height)
    } else {
        if !matches!(image.kind, ImageKind32::Icon | ImageKind32::Cursor) || visual.style & 31 != 3
        {
            return Err(GdiError32::InvalidRequest);
        }
        let info = gdi.image_info(image.handle)?;
        (info.width, info.height)
    };
    let centered = visual.style & 0x200 != 0;
    let width = i32::try_from(width).map_err(|_| GdiError32::InvalidRequest)?;
    let height = i32::try_from(height).map_err(|_| GdiError32::InvalidRequest)?;
    let destination = if centered {
        let left = bounds.left + (bounds.right - bounds.left - width) / 2;
        let top = bounds.top + (bounds.bottom - bounds.top - height) / 2;
        Rect32 {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    } else if visual.style & 0x40 != 0 {
        bounds
    } else {
        Rect32 {
            right: bounds.left + width,
            bottom: bounds.top + height,
            ..bounds
        }
    };
    if image.kind == ImageKind32::Bitmap {
        canvas.bitmap(gdi, memory, image.handle, destination, centered)
    } else {
        canvas.image(
            gdi,
            GdiImageDraw32 {
                handle: image.handle,
                position: Point32 {
                    x: destination.left,
                    y: destination.top,
                },
                width: (destination.right - destination.left) as u32,
                height: (destination.bottom - destination.top) as u32,
                flags: 3,
                background: 0,
            },
        )
    }
}
