use super::super::storage::{Storage32, SurfaceKey32};
use super::super::{bitmaps::bounds, context::GdiRuntime32, memory};
use crate::{
    memory::AddressSpace,
    windows::{
        gdi::{
            Brush32, DcState32, GdiApi32, GdiError32, GdiObject32, raster_operation32,
            raster_uses_source32,
        },
        user32::{Point32, Rect32},
    },
};

pub(in crate::process::windows::gdi) struct BlitImage32 {
    pub layout: crate::windows::gdi::BitmapLayout32,
    pub palette: Vec<u32>,
    pub pixels: Vec<u8>,
    pub start: Point32,
    pub end: Point32,
    pub background: u32,
}
pub(in crate::process::windows::gdi) struct BlitRequest32 {
    pub dc: u32,
    pub origin: Point32,
    pub end: Point32,
    pub source: Option<BlitImage32>,
    pub rop: u32,
    pub pattern_only: bool,
}

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn fill_color_rect(
        &mut self,
        memory: &mut AddressSpace,
        dc: u32,
        rect: Rect32,
        brush: u32,
    ) -> Result<(), GdiError32> {
        let index = self.dc_surface(dc)?;
        let layout = self.surfaces[index].layout;
        let state = self.model.state(dc)?.clone();
        let area = intersect(rect, bounds(layout));
        let mut pixels = self.surfaces[index]
            .storage
            .read(memory, layout.byte_len()?)?;
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let point = Point32 { x, y };
                if !state.accepts(point, bounds(layout)) {
                    continue;
                }
                let color = if (1..=31).contains(&brush) {
                    Some(self.system_color(brush - 1))
                } else {
                    self.brush(&state, brush, point)?
                };
                if let Some(color) = color {
                    layout.write_pixel(
                        &mut pixels,
                        &self.surfaces[index].palette,
                        x as u32,
                        y as u32,
                        color,
                    );
                }
            }
        }
        if area.left < area.right && area.top < area.bottom {
            self.commit_pixels(memory, index, pixels, area)?;
        }
        Ok(())
    }
    pub(in crate::process::windows::gdi) fn brush(
        &self,
        state: &DcState32,
        handle: u32,
        point: Point32,
    ) -> Result<Option<u32>, GdiError32> {
        if handle == crate::windows::gdi::GdiContext32::stock_object(18)? {
            return Ok(Some(state.dc_brush_color));
        }
        let GdiObject32::Brush(brush) = self.model.object(handle)? else {
            return Err(GdiError32::InvalidHandle);
        };
        self.brush_value(state, handle, brush, point)
    }
    pub(in crate::process::windows::gdi) fn brush_value(
        &self,
        state: &DcState32,
        handle: u32,
        brush: Brush32,
        point: Point32,
    ) -> Result<Option<u32>, GdiError32> {
        Ok(match brush {
            Brush32::Solid(color) => Some(color),
            Brush32::System(index) => Some(self.system_color(index)),
            Brush32::Null => None,
            Brush32::Pattern { monochrome, .. } => {
                let surface =
                    &self.surfaces[self.surface_index(SurfaceKey32::PatternBrush(handle))?];
                let Storage32::Owned(pixels) = &surface.storage else {
                    return Err(GdiError32::InvalidRequest);
                };
                let x = (i64::from(point.x) - i64::from(state.brush_origin.x))
                    .rem_euclid(i64::from(surface.layout.width)) as u32;
                let y = (i64::from(point.y) - i64::from(state.brush_origin.y))
                    .rem_euclid(i64::from(surface.layout.height)) as u32;
                Some(if monochrome {
                    let bit = pixels[surface.layout.row(y) + x as usize / 8] >> (7 - x % 8) & 1;
                    if bit == 0 {
                        state.text_color
                    } else {
                        state.background_color
                    }
                } else {
                    surface.layout.read_pixel(pixels, &surface.palette, x, y)
                })
            }
            Brush32::Hatch { style, color } => {
                let x = (i64::from(point.x) - i64::from(state.brush_origin.x)).rem_euclid(8);
                let y = (i64::from(point.y) - i64::from(state.brush_origin.y)).rem_euclid(8);
                if match style {
                    0 => y == 0,
                    1 => x == 0,
                    2 => (x + y) % 8 == 0,
                    3 => (x - y).rem_euclid(8) == 0,
                    4 => x == 0 || y == 0,
                    5 => (x + y) % 8 == 0 || (x - y).rem_euclid(8) == 0,
                    _ => false,
                } {
                    Some(color)
                } else if state.background_mode == 2 {
                    Some(state.background_color)
                } else {
                    None
                }
            }
        })
    }
    pub(in crate::process::windows::gdi) fn blit(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let state = self.model.state(args[0])?.clone();
        let pat = api == GdiApi32::PatBlt;
        let stretch = api == GdiApi32::StretchBlt;
        let rop = args[if pat {
            5
        } else if stretch {
            10
        } else {
            8
        }];
        if rop & 0xc000_0000 != 0 {
            return Err(GdiError32::Unsupported);
        }
        if pat && raster_uses_source32(rop) {
            return Err(GdiError32::InvalidRequest);
        }
        let width = args[3] as i32;
        let height = args[4] as i32;
        if width == 0 || height == 0 {
            return Ok(1);
        }
        let origin = state.device_point(Point32 {
            x: args[1] as i32,
            y: args[2] as i32,
        })?;
        let end = state.device_point(Point32 {
            x: (args[1] as i32)
                .checked_add(width)
                .ok_or(GdiError32::InvalidRequest)?,
            y: (args[2] as i32)
                .checked_add(height)
                .ok_or(GdiError32::InvalidRequest)?,
        })?;
        let source = if !pat && raster_uses_source32(rop) {
            let source = self.dc_surface(args[5])?;
            let source_state = self.model.state(args[5])?;
            let surface = &self.surfaces[source];
            let source_width = if stretch { args[8] as i32 } else { width };
            let source_height = if stretch { args[9] as i32 } else { height };
            if source_width == 0 || source_height == 0 {
                return Err(GdiError32::InvalidRequest);
            }
            let start = source_state.device_point(Point32 {
                x: args[6] as i32,
                y: args[7] as i32,
            })?;
            let end = source_state.device_point(Point32 {
                x: (args[6] as i32)
                    .checked_add(source_width)
                    .ok_or(GdiError32::InvalidRequest)?,
                y: (args[7] as i32)
                    .checked_add(source_height)
                    .ok_or(GdiError32::InvalidRequest)?,
            })?;
            Some(BlitImage32 {
                layout: surface.layout,
                palette: surface.palette.clone(),
                pixels: surface.storage.read(memory, surface.layout.byte_len()?)?,
                start,
                end,
                background: source_state.background_color,
            })
        } else {
            None
        };
        self.render_blit(
            memory,
            BlitRequest32 {
                dc: args[0],
                origin,
                end,
                source,
                rop,
                pattern_only: pat,
            },
        )
    }
    pub(in crate::process::windows::gdi) fn render_blit(
        &mut self,
        memory: &mut AddressSpace,
        request: BlitRequest32,
    ) -> Result<u32, GdiError32> {
        let BlitRequest32 {
            dc,
            origin,
            end,
            source,
            rop,
            pattern_only: pat,
        } = request;
        let target = self.dc_surface(dc)?;
        let state = self.model.state(dc)?.clone();
        let target_layout = self.surfaces[target].layout;
        let mut pixels = self.surfaces[target]
            .storage
            .read(memory, target_layout.byte_len()?)?;
        let area = intersect(
            Rect32 {
                left: origin.x.min(end.x),
                top: origin.y.min(end.y),
                right: origin.x.max(end.x),
                bottom: origin.y.max(end.y),
            },
            bounds(target_layout),
        );
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let point = Point32 { x, y };
                if !state.accepts(point, bounds(target_layout)) {
                    continue;
                }
                let pattern = self.brush(&state, state.brush, point)?.unwrap_or(0);
                if pat && self.brush(&state, state.brush, point)?.is_none() {
                    continue;
                }
                let mut source_color = 0;
                if let Some(image) = source.as_ref() {
                    let layout = image.layout;
                    let palette = &image.palette;
                    let Some(color) = super::stretch::sample(
                        image,
                        point,
                        origin,
                        end,
                        state.stretch_mode,
                        (state.stretch_mode == 4 && target_layout.bits_per_pixel != 1)
                            .then_some((state.text_color, state.background_color)),
                    )?
                    else {
                        continue;
                    };
                    source_color = color;
                    if layout.bits_per_pixel == 1
                        && target_layout.bits_per_pixel != 1
                        && state.stretch_mode != 4
                    {
                        source_color = if source_color == palette.first().copied().unwrap_or(0) {
                            state.text_color
                        } else {
                            state.background_color
                        };
                    } else if target_layout.bits_per_pixel == 1
                        && layout.bits_per_pixel != 1
                        && state.stretch_mode != 4
                    {
                        source_color = if source_color & 0xffffff == image.background {
                            0x00ff_ffff
                        } else {
                            0
                        };
                    }
                }
                if state.stretch_mode == 4 && target_layout.bits_per_pixel <= 8 {
                    source_color = super::stretch::quantized(
                        source_color,
                        &self.surfaces[target].palette,
                        point,
                        state.brush_origin,
                    );
                }
                let destination = target_layout.read_pixel(
                    &pixels,
                    &self.surfaces[target].palette,
                    x as u32,
                    y as u32,
                );
                let color = raster_operation32(rop, pattern, source_color, destination);
                target_layout.write_pixel(
                    &mut pixels,
                    &self.surfaces[target].palette,
                    x as u32,
                    y as u32,
                    color,
                );
                if rop & 0x00ff_0000 == 0x00cc_0000
                    && target_layout.bits_per_pixel == 32
                    && target_layout.masks.is_none()
                    && source.as_ref().is_some_and(|image| {
                        image.layout.bits_per_pixel == 32 && image.layout.masks.is_none()
                    })
                {
                    pixels[target_layout.row(y as u32) + x as usize * 4 + 3] =
                        (source_color >> 24) as u8;
                }
            }
        }
        if area.left < area.right && area.top < area.bottom {
            self.commit_pixels(memory, target, pixels, area)?;
        }
        Ok(1)
    }
    pub(in crate::process::windows::gdi) fn rectangle(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(args[0])?;
        let layout = self.surfaces[index].layout;
        let state = self.model.state(args[0])?.clone();
        let logical = if api == GdiApi32::Rectangle {
            Rect32 {
                left: args[1] as i32,
                top: args[2] as i32,
                right: args[3] as i32,
                bottom: args[4] as i32,
            }
        } else {
            memory::rect(memory, args[1])?
        };
        let a = state.device_point(Point32 {
            x: logical.left,
            y: logical.top,
        })?;
        let b = state.device_point(Point32 {
            x: logical.right,
            y: logical.bottom,
        })?;
        let rect = Rect32 {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x),
            bottom: a.y.max(b.y),
        };
        let area = intersect(rect, bounds(layout));
        let brush = if matches!(api, GdiApi32::FillRect | GdiApi32::FrameRect) {
            args[2]
        } else {
            state.brush
        };
        if api != GdiApi32::InvertRect {
            self.brush(&state, brush, a)?;
        }
        let pen = if api == GdiApi32::Rectangle {
            let GdiObject32::Pen(pen) = self.model.object(state.pen)? else {
                return Err(GdiError32::InvalidHandle);
            };
            if !matches!(pen.style & 15, 0 | 5 | 6) || pen.extended.is_some() && pen.width > 1 {
                return Err(GdiError32::Unsupported);
            }
            Some(pen)
        } else {
            None
        };
        let mut pixels = self.surfaces[index]
            .storage
            .read(memory, layout.byte_len()?)?;
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let point = Point32 { x, y };
                if !state.accepts(point, bounds(layout)) {
                    continue;
                }
                let border =
                    x == rect.left || x == rect.right - 1 || y == rect.top || y == rect.bottom - 1;
                if api == GdiApi32::FrameRect && !border {
                    continue;
                }
                let color = if api == GdiApi32::InvertRect {
                    Some(!layout.read_pixel(
                        &pixels,
                        &self.surfaces[index].palette,
                        x as u32,
                        y as u32,
                    ))
                } else if let Some(pen) = pen
                    && pen.style & 15 != 5
                    && (x - rect.left < pen.width
                        || rect.right - x <= pen.width
                        || y - rect.top < pen.width
                        || rect.bottom - y <= pen.width)
                {
                    self.pen_color(&state, state.pen, pen, point)?
                        .or(self.brush(&state, brush, point)?)
                } else {
                    self.brush(&state, brush, point)?
                };
                if let Some(color) = color {
                    let color = if api == GdiApi32::Rectangle {
                        let old = layout.read_pixel(
                            &pixels,
                            &self.surfaces[index].palette,
                            x as u32,
                            y as u32,
                        );
                        binary_rop(state.rop2, color, old)
                    } else {
                        color
                    };
                    layout.write_pixel(
                        &mut pixels,
                        &self.surfaces[index].palette,
                        x as u32,
                        y as u32,
                        color,
                    );
                }
            }
        }
        if area.left < area.right && area.top < area.bottom {
            self.commit_pixels(memory, index, pixels, area)?;
        }
        Ok(1)
    }
}
pub(in crate::process::windows::gdi) fn intersect(a: Rect32, b: Rect32) -> Rect32 {
    Rect32 {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    }
}
pub(in crate::process::windows::gdi) fn binary_rop(mode: u32, pen: u32, destination: u32) -> u32 {
    let mut result = 0;
    let table = mode - 1;
    for index in 0..4 {
        if table & (1 << index) != 0 {
            result |= (if index & 2 == 0 { !pen } else { pen })
                & (if index & 1 == 0 {
                    !destination
                } else {
                    destination
                });
        }
    }
    result
}
