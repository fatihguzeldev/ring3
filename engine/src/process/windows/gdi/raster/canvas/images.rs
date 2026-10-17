use super::{GdiCanvas32, union};
use crate::{
    process::windows::gdi::{GdiImageDraw32, GdiRuntime32},
    windows::{
        gdi::GdiError32,
        user32::{Point32, Rect32},
    },
};

impl GdiCanvas32 {
    pub(in crate::process) fn image(
        &mut self,
        gdi: &GdiRuntime32,
        request: GdiImageDraw32,
    ) -> Result<(), GdiError32> {
        if request.flags & !0x1f != 0 || request.flags & 3 == 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let witness = gdi.image_owner(request.handle)?;
        let image = gdi.image_info(request.handle)?;
        let prepared = self.prepared.as_mut().ok_or(GdiError32::InvalidRequest)?;
        let surface = gdi
            .surfaces
            .get(prepared.index)
            .ok_or(GdiError32::InvalidRequest)?;
        if surface.id != self.owner.surface_id
            || surface.revision != self.owner.surface_revision
            || gdi.dc_surface(self.dc)? != prepared.index
            || gdi.model.state(self.dc)? != &self.owner.state
        {
            return Err(GdiError32::InvalidRequest);
        }
        let width = if request.width == 0 {
            image.width
        } else {
            request.width
        };
        let height = if request.height == 0 {
            image.height
        } else {
            request.height
        };
        let width = i32::try_from(width).map_err(|_| GdiError32::InvalidRequest)?;
        let height = i32::try_from(height).map_err(|_| GdiError32::InvalidRequest)?;
        let start = prepared.state.device_point(request.position)?;
        let end = prepared.state.device_point(Point32 {
            x: request
                .position
                .x
                .checked_add(width)
                .ok_or(GdiError32::InvalidRequest)?,
            y: request
                .position
                .y
                .checked_add(height)
                .ok_or(GdiError32::InvalidRequest)?,
        })?;
        if start.x == end.x || start.y == end.y {
            return Ok(());
        }
        if request.background != 0 {
            gdi.brush(&prepared.state, request.background, start)?;
        }
        if !self
            .images
            .iter()
            .any(|source| source.handle == request.handle)
        {
            self.images
                .try_reserve(1)
                .map_err(|_| GdiError32::Allocation)?;
            self.images.push(witness);
        }
        let bounds = super::super::super::bitmaps::bounds(surface.layout);
        let area = Rect32 {
            left: start.x.min(end.x).max(bounds.left),
            top: start.y.min(end.y).max(bounds.top),
            right: start.x.max(end.x).min(bounds.right),
            bottom: start.y.max(end.y).min(bounds.bottom),
        };
        let source_axis = |position: i32, first: i32, last: i32, size: u32| {
            let local = if last > first {
                i64::from(position) - i64::from(first)
            } else {
                i64::from(first) - i64::from(position) - 1
            };
            (local as u64 * u64::from(size) / (i64::from(last) - i64::from(first)).unsigned_abs())
                as u32
        };
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let point = Point32 { x, y };
                if !prepared.state.accepts(point, bounds) {
                    continue;
                }
                let sx = source_axis(x, start.x, end.x, image.width);
                let sy = source_axis(y, start.y, end.y, image.height);
                let mask =
                    image
                        .mask
                        .layout
                        .read_pixel(image.mask.pixels, image.mask.palette, sx, sy);
                let color = if let Some(color) = image.color {
                    color.layout.read_pixel(color.pixels, color.palette, sx, sy)
                } else {
                    image.mask.layout.read_pixel(
                        image.mask.pixels,
                        image.mask.palette,
                        sx,
                        sy + image.height,
                    )
                };
                let raw_alpha = image.color.map_or(0, |color| {
                    color.pixels[color.layout.row(sy) + sx as usize * 4 + 3]
                });
                let mut destination = surface.layout.read_pixel(
                    &prepared.pixels,
                    &surface.palette,
                    x as u32,
                    y as u32,
                );
                let mut destination_alpha =
                    if surface.layout.bits_per_pixel == 32 && surface.layout.masks.is_none() {
                        prepared.pixels[surface.layout.row(y as u32) + x as usize * 4 + 3]
                    } else {
                        0
                    };
                if request.background != 0
                    && let Some(background) =
                        gdi.brush(&prepared.state, request.background, point)?
                {
                    destination = background;
                    destination_alpha = 0;
                }
                let (value, alpha) = match request.flags & 3 {
                    1 => (mask, 0),
                    2 => (color, raw_alpha),
                    3 if image.alpha => {
                        let alpha = u32::from(raw_alpha);
                        let blend = |shift: u32| -> u32 {
                            (((color >> shift) & 255) * alpha
                                + ((destination >> shift) & 255) * (255 - alpha))
                                / 255
                        };
                        (
                            blend(0) | (blend(8) << 8) | (blend(16) << 16),
                            (alpha + u32::from(destination_alpha) * (255 - alpha) / 255) as u8,
                        )
                    }
                    3 => ((destination & mask) ^ color, 0),
                    _ => unreachable!(),
                };
                surface.layout.write_pixel(
                    &mut prepared.pixels,
                    &surface.palette,
                    x as u32,
                    y as u32,
                    value,
                );
                if surface.layout.bits_per_pixel == 32 && surface.layout.masks.is_none() {
                    prepared.pixels[surface.layout.row(y as u32) + x as usize * 4 + 3] = alpha;
                }
                prepared.damage = union(
                    prepared.damage,
                    Rect32 {
                        left: x,
                        top: y,
                        right: x + 1,
                        bottom: y + 1,
                    },
                );
            }
        }
        Ok(())
    }
}
