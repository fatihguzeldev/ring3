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
        Ok(match brush {
            Brush32::Solid(color) => Some(color),
            Brush32::System(index) => Some(self.system_color(index)),
            Brush32::Null => None,
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
                if let Some(BlitImage32 {
                    layout,
                    palette,
                    pixels: bytes,
                    start: from,
                    end: to,
                    background,
                }) = source.as_ref()
                {
                    let sx = scale_pixel(x, origin.x, end.x, from.x, to.x)?;
                    let sy = scale_pixel(y, origin.y, end.y, from.y, to.y)?;
                    if sx < 0 || sy < 0 || sx as u32 >= layout.width || sy as u32 >= layout.height {
                        continue;
                    }
                    source_color = layout.read_pixel(bytes, palette, sx as u32, sy as u32);
                    if layout.bits_per_pixel == 1 && target_layout.bits_per_pixel != 1 {
                        source_color = if source_color == palette.first().copied().unwrap_or(0) {
                            state.text_color
                        } else {
                            state.background_color
                        };
                    } else if target_layout.bits_per_pixel == 1 && layout.bits_per_pixel != 1 {
                        source_color = if source_color == *background {
                            0x00ff_ffff
                        } else {
                            0
                        };
                    }
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
            if !matches!(pen.style, 0 | 5 | 6) {
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
                    && pen.style != 5
                    && (x - rect.left < pen.width
                        || rect.right - x <= pen.width
                        || y - rect.top < pen.width
                        || rect.bottom - y <= pen.width)
                {
                    Some(
                        if state.pen == crate::windows::gdi::GdiContext32::stock_object(19)? {
                            state.dc_pen_color
                        } else {
                            pen.color
                        },
                    )
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
    pub(in crate::process::windows::gdi) fn line(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(args[0])?;
        let layout = self.surfaces[index].layout;
        let mut state = self.model.state(args[0])?.clone();
        let end_logical = Point32 {
            x: args[1] as i32,
            y: args[2] as i32,
        };
        let mut point = state.device_point(state.current_position)?;
        let end = state.device_point(end_logical)?;
        let GdiObject32::Pen(pen) = self.model.object(state.pen)? else {
            return Err(GdiError32::InvalidHandle);
        };
        if !matches!(pen.style, 0 | 5) || pen.width != 1 {
            return Err(GdiError32::Unsupported);
        }
        let dx = (i64::from(end.x) - i64::from(point.x)).abs();
        let dy = (i64::from(end.y) - i64::from(point.y)).abs();
        if dx.max(dy) > 1_048_576 {
            return Err(GdiError32::Capacity);
        }
        let mut pixels = self.surfaces[index]
            .storage
            .read(memory, layout.byte_len()?)?;
        let mut error = dx - dy;
        let sx = if point.x < end.x { 1 } else { -1 };
        let sy = if point.y < end.y { 1 } else { -1 };
        let color = if state.pen == crate::windows::gdi::GdiContext32::stock_object(19)? {
            state.dc_pen_color
        } else {
            pen.color
        };
        while point != end {
            if pen.style != 5 && state.accepts(point, bounds(layout)) {
                let old = layout.read_pixel(
                    &pixels,
                    &self.surfaces[index].palette,
                    point.x as u32,
                    point.y as u32,
                );
                let value = binary_rop(state.rop2, color, old);
                layout.write_pixel(
                    &mut pixels,
                    &self.surfaces[index].palette,
                    point.x as u32,
                    point.y as u32,
                    value,
                );
            }
            let twice = 2 * error;
            if twice > -dy {
                error -= dy;
                point.x += sx;
            }
            if twice < dx {
                error += dx;
                point.y += sy;
            }
        }
        self.commit_pixels(memory, index, pixels, bounds(layout))?;
        state.current_position = end_logical;
        self.model.set_state(args[0], state)?;
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
fn scale_pixel(
    value: i32,
    first: i32,
    last: i32,
    source: i32,
    end: i32,
) -> Result<i32, GdiError32> {
    let extent = i64::from(last) - i64::from(first);
    if extent == 0 {
        return Err(GdiError32::InvalidRequest);
    }
    let index = if extent > 0 {
        i64::from(value) - i64::from(first)
    } else {
        i64::from(first) - i64::from(value) - 1
    };
    let source_extent = i64::from(end) - i64::from(source);
    let offset = index * source_extent.abs() / extent.abs();
    i32::try_from(if source_extent >= 0 {
        i64::from(source) + offset
    } else {
        i64::from(source) - offset - 1
    })
    .map_err(|_| GdiError32::InvalidRequest)
}
fn binary_rop(mode: u32, pen: u32, destination: u32) -> u32 {
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
