use super::{CanvasSource32, GdiCanvas32, intersect, union};
use crate::{
    memory::AddressSpace,
    process::windows::gdi::GdiRuntime32,
    windows::{
        gdi::{BitmapLayout32, DcState32, GdiError32},
        user32::{Point32, Rect32},
    },
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::gdi) enum CompositeMode32 {
    Alpha { constant: u8, pixel_alpha: bool },
    Transparent(u32),
}
pub(in crate::process::windows::gdi) struct CompositeRequest32 {
    pub source: u32,
    pub source_area: Rect32,
    pub destination: Rect32,
    pub mode: CompositeMode32,
}

impl GdiCanvas32 {
    pub(in crate::process::windows::gdi) fn composite(
        &mut self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        request: CompositeRequest32,
    ) -> Result<(), GdiError32> {
        let prepared = self.prepared.as_ref().ok_or(GdiError32::InvalidRequest)?;
        self.owner.check(gdi, memory, self.dc, prepared.index)?;
        let source = Self::new(gdi, memory, request.source)?;
        let input = source.prepared.as_ref().unwrap();
        let source_area = device_area(&input.state, request.source_area)?;
        let destination = device_area(&prepared.state, request.destination)?;
        let source_bounds = super::super::super::bitmaps::bounds(source.owner.layout);
        if matches!(request.mode, CompositeMode32::Alpha { .. }) {
            if source_area.left < source_bounds.left
                || source_area.top < source_bounds.top
                || source_area.right > source_bounds.right
                || source_area.bottom > source_bounds.bottom
                || (self.owner.surface_id == source.owner.surface_id
                    && !empty(intersect(source_area, destination)))
            {
                return Err(GdiError32::InvalidRequest);
            }
            if matches!(
                request.mode,
                CompositeMode32::Alpha {
                    pixel_alpha: true,
                    ..
                }
            ) && source.owner.layout.bits_per_pixel != 32
            {
                return Err(GdiError32::InvalidRequest);
            }
        }
        if self.sources.len() >= gdi.config.dcs {
            return Err(GdiError32::Capacity);
        }
        self.sources
            .try_reserve(1)
            .map_err(|_| GdiError32::Allocation)?;
        let bounds = super::super::super::bitmaps::bounds(self.owner.layout);
        let output_area = intersect(destination, bounds);
        let prepared = self.prepared.as_mut().unwrap();
        for y in output_area.top..output_area.bottom {
            for x in output_area.left..output_area.right {
                if !prepared.state.accepts(Point32 { x, y }, bounds) {
                    continue;
                }
                let sx = sample(
                    x,
                    destination.left,
                    destination.right,
                    source_area.left,
                    source_area.right,
                );
                let sy = sample(
                    y,
                    destination.top,
                    destination.bottom,
                    source_area.top,
                    source_area.bottom,
                );
                if sx < 0 || sy < 0 || sx >= source_bounds.right || sy >= source_bounds.bottom {
                    continue;
                }
                let source_color = source.owner.layout.read_pixel(
                    &input.pixels,
                    &source.owner.colors,
                    sx as u32,
                    sy as u32,
                );
                let source_alpha = alpha(source.owner.layout, &input.pixels, sx as u32, sy as u32);
                let (color, alpha) = match request.mode {
                    CompositeMode32::Transparent(key) => {
                        if source_color == key & 0x00ff_ffff {
                            continue;
                        }
                        (source_color, 0)
                    }
                    CompositeMode32::Alpha {
                        constant,
                        pixel_alpha,
                    } => {
                        let destination_color = self.owner.layout.read_pixel(
                            &prepared.pixels,
                            &self.owner.colors,
                            x as u32,
                            y as u32,
                        );
                        let destination_alpha =
                            alpha(self.owner.layout, &prepared.pixels, x as u32, y as u32);
                        let opacity = if pixel_alpha {
                            scale(source_alpha, constant)
                        } else {
                            constant
                        };
                        let blend = |shift: u32| {
                            let source = scale(((source_color >> shift) & 255) as u8, constant);
                            let destination =
                                scale(((destination_color >> shift) & 255) as u8, 255 - opacity);
                            u32::from(source.saturating_add(destination))
                        };
                        let source_alpha = if pixel_alpha {
                            opacity
                        } else {
                            scale(source_alpha, constant)
                        };
                        (
                            blend(0) | blend(8) << 8 | blend(16) << 16,
                            source_alpha.saturating_add(scale(destination_alpha, 255 - opacity)),
                        )
                    }
                };
                self.owner.layout.write_pixel(
                    &mut prepared.pixels,
                    &self.owner.colors,
                    x as u32,
                    y as u32,
                    color,
                );
                write_alpha(
                    self.owner.layout,
                    &mut prepared.pixels,
                    x as u32,
                    y as u32,
                    alpha,
                );
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
        self.sources.push(CanvasSource32 {
            dc: source.dc,
            index: input.index,
            owner: source.owner,
        });
        Ok(())
    }
}

fn sample(value: i32, first: i32, end: i32, source: i32, source_end: i32) -> i32 {
    (i128::from(source)
        + (i128::from(value) - i128::from(first)) * (i128::from(source_end) - i128::from(source))
            / (i128::from(end) - i128::from(first))) as i32
}
fn device_area(state: &DcState32, rect: Rect32) -> Result<Rect32, GdiError32> {
    if empty(rect) {
        return Err(GdiError32::InvalidRequest);
    }
    let first = state.device_point(Point32 {
        x: rect.left,
        y: rect.top,
    })?;
    let end = state.device_point(Point32 {
        x: rect.right,
        y: rect.bottom,
    })?;
    let result = Rect32 {
        left: first.x,
        top: first.y,
        right: end.x,
        bottom: end.y,
    };
    if empty(result) {
        return Err(GdiError32::InvalidRequest);
    }
    Ok(result)
}
fn empty(rect: Rect32) -> bool {
    rect.left >= rect.right || rect.top >= rect.bottom
}
fn scale(value: u8, alpha: u8) -> u8 {
    ((u32::from(value) * u32::from(alpha) + 127) / 255) as u8
}
fn alpha(layout: BitmapLayout32, bytes: &[u8], x: u32, y: u32) -> u8 {
    if layout.bits_per_pixel == 32 && layout.masks.is_none() {
        bytes[layout.row(y) + x as usize * 4 + 3]
    } else {
        0
    }
}
pub(super) fn write_alpha(layout: BitmapLayout32, bytes: &mut [u8], x: u32, y: u32, alpha: u8) {
    if layout.bits_per_pixel == 32 && layout.masks.is_none() {
        bytes[layout.row(y) + x as usize * 4 + 3] = alpha;
    }
}

pub(super) struct CanvasBitmap32 {
    handle: u32,
    id: u64,
    revision: u64,
    layout: BitmapLayout32,
    palette: Vec<u32>,
}
impl CanvasBitmap32 {
    pub(super) fn check(
        &self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
    ) -> Result<(), GdiError32> {
        gdi.bitmap_info(memory, self.handle)?;
        let index = gdi.surface_index(super::super::super::storage::SurfaceKey32::Bitmap(
            self.handle,
        ))?;
        let source = &gdi.surfaces[index];
        if source.id != self.id
            || source.revision != self.revision
            || source.layout != self.layout
            || source.palette != self.palette
        {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
}
impl GdiCanvas32 {
    pub(in crate::process::windows::gdi) fn bitmap(
        &mut self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        handle: u32,
        destination: Rect32,
        centered_background: bool,
    ) -> Result<(), GdiError32> {
        let prepared = self.prepared.as_ref().ok_or(GdiError32::InvalidRequest)?;
        self.owner.check(gdi, memory, self.dc, prepared.index)?;
        gdi.bitmap_info(memory, handle)?;
        let source_index =
            gdi.surface_index(super::super::super::storage::SurfaceKey32::Bitmap(handle))?;
        let source = &gdi.surfaces[source_index];
        let pixels = source.storage.read(memory, source.layout.byte_len()?)?;
        if self.bitmaps.len() >= gdi.config.objects {
            return Err(GdiError32::Capacity);
        }
        self.bitmaps
            .try_reserve(1)
            .map_err(|_| GdiError32::Allocation)?;
        let mut palette = Vec::new();
        palette
            .try_reserve_exact(source.palette.len())
            .map_err(|_| GdiError32::Allocation)?;
        palette.extend_from_slice(&source.palette);
        let witness = CanvasBitmap32 {
            handle,
            id: source.id,
            revision: source.revision,
            layout: source.layout,
            palette,
        };
        let bounds = super::super::super::bitmaps::bounds(self.owner.layout);
        if centered_background {
            self.fill(
                gdi,
                bounds,
                source.layout.read_pixel(&pixels, &source.palette, 0, 0),
            );
        }
        if empty(destination) {
            self.bitmaps.push(witness);
            return Ok(());
        }
        let destination = device_area(&self.prepared.as_ref().unwrap().state, destination)?;
        let area = intersect(destination, bounds);
        let prepared = self.prepared.as_mut().unwrap();
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                if !prepared.state.accepts(Point32 { x, y }, bounds) {
                    continue;
                }
                let sx = sample(
                    x,
                    destination.left,
                    destination.right,
                    0,
                    source.layout.width as i32,
                ) as u32;
                let sy = sample(
                    y,
                    destination.top,
                    destination.bottom,
                    0,
                    source.layout.height as i32,
                ) as u32;
                let color = source.layout.read_pixel(&pixels, &source.palette, sx, sy);
                self.owner.layout.write_pixel(
                    &mut prepared.pixels,
                    &self.owner.colors,
                    x as u32,
                    y as u32,
                    color,
                );
                write_alpha(
                    self.owner.layout,
                    &mut prepared.pixels,
                    x as u32,
                    y as u32,
                    0,
                );
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
        self.bitmaps.push(witness);
        Ok(())
    }
}
