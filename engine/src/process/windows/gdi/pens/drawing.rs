use super::super::raster::binary_rop;
use super::super::{bitmaps::bounds, context::GdiRuntime32};
use crate::{
    memory::AddressSpace,
    windows::{
        gdi::{DcState32, GdiError32, GdiObject32, Pen32},
        user32::{Point32, Rect32},
    },
};

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn pen_color(
        &self,
        state: &DcState32,
        handle: u32,
        pen: Pen32,
        point: Point32,
    ) -> Result<Option<u32>, GdiError32> {
        if let Some(extended) = pen.extended {
            return self.brush_value(state, handle, extended.brush, point);
        }
        Ok(Some(
            if handle == crate::windows::gdi::GdiContext32::stock_object(19)? {
                state.dc_pen_color
            } else {
                pen.color
            },
        ))
    }

    pub(in crate::process::windows::gdi) fn line(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(args[0])?;
        let surface = &self.surfaces[index];
        let layout = surface.layout;
        let mut state = self.model.state(args[0])?.clone();
        let logical_end = Point32 {
            x: args[1] as i32,
            y: args[2] as i32,
        };
        let first = state.device_point(state.current_position)?;
        let end = state.device_point(logical_end)?;
        let GdiObject32::Pen(pen) = self.model.object(state.pen)? else {
            return Err(GdiError32::InvalidHandle);
        };
        let geometric = pen.style & 0x10000 != 0 || pen.width > 1;
        let dx = i64::from(end.x) - i64::from(first.x);
        let dy = i64::from(end.y) - i64::from(first.y);
        if dx.abs().max(dy.abs()) > 1_048_576 {
            return Err(GdiError32::Capacity);
        }
        if pen.style & 15 == 5
            || first == end
            || pen
                .extended
                .is_some_and(|extended| extended.brush == crate::windows::gdi::Brush32::Null)
        {
            state.current_position = logical_end;
            self.model.set_state(args[0], state)?;
            return Ok(1);
        }
        let mut pixels = surface.storage.read(memory, layout.byte_len()?)?;
        let mut damage = Rect32::default();
        let mut draw = |point: Point32, position: f64| -> Result<(), GdiError32> {
            if !state.accepts(point, bounds(layout)) {
                return Ok(());
            }
            let on = dash_on(pen, position);
            let color = if on {
                self.pen_color(&state, state.pen, pen, point)?
            } else if pen.extended.is_none() && state.background_mode == 2 {
                Some(state.background_color)
            } else {
                None
            };
            let Some(color) = color else {
                return Ok(());
            };
            let old = layout.read_pixel(&pixels, &surface.palette, point.x as u32, point.y as u32);
            layout.write_pixel(
                &mut pixels,
                &surface.palette,
                point.x as u32,
                point.y as u32,
                binary_rop(state.rop2, color, old),
            );
            let pixel = Rect32 {
                left: point.x,
                top: point.y,
                right: point.x + 1,
                bottom: point.y + 1,
            };
            damage = if damage.left >= damage.right || damage.top >= damage.bottom {
                pixel
            } else {
                Rect32 {
                    left: damage.left.min(pixel.left),
                    top: damage.top.min(pixel.top),
                    right: damage.right.max(pixel.right),
                    bottom: damage.bottom.max(pixel.bottom),
                }
            };
            Ok(())
        };
        if pen.style & 15 != 5 && first != end {
            if !geometric {
                let mut point = first;
                let (ax, ay) = (dx.abs(), dy.abs());
                let mut error = ax - ay;
                let mut step = 0u32;
                while point != end {
                    draw(point, f64::from(step))?;
                    let twice = 2 * error;
                    if twice > -ay {
                        error -= ay;
                        point.x += if dx > 0 { 1 } else { -1 };
                    }
                    if twice < ax {
                        error += ax;
                        point.y += if dy > 0 { 1 } else { -1 };
                    }
                    step += 1;
                }
            } else {
                let width = f64::from(pen.width);
                let wx = (width * f64::from(state.viewport_extent.x)
                    / f64::from(state.window_extent.x))
                .abs()
                .max(1.0);
                let wy = (width * f64::from(state.viewport_extent.y)
                    / f64::from(state.window_extent.y))
                .abs()
                .max(1.0);
                let (rx, ry) = (wx / 2.0, wy / 2.0);
                let nx = dx as f64 / rx;
                let ny = dy as f64 / ry;
                let length2 = nx * nx + ny * ny;
                let logical_dx = f64::from(logical_end.x) - f64::from(state.current_position.x);
                let logical_dy = f64::from(logical_end.y) - f64::from(state.current_position.y);
                let logical_length = logical_dx.hypot(logical_dy);
                let bounds = bounds(layout);
                let left = ((f64::from(first.x.min(end.x)) - rx).floor() as i64)
                    .max(i64::from(bounds.left));
                let right = ((f64::from(first.x.max(end.x)) + rx).ceil() as i64)
                    .min(i64::from(bounds.right));
                let top = ((f64::from(first.y.min(end.y)) - ry).floor() as i64)
                    .max(i64::from(bounds.top));
                let bottom = ((f64::from(first.y.max(end.y)) + ry).ceil() as i64)
                    .min(i64::from(bounds.bottom));
                let ox = if wx.round() as u64 % 2 == 0 { 0.5 } else { 0.0 };
                let oy = if wy.round() as u64 % 2 == 0 { 0.5 } else { 0.0 };
                for y in top..bottom {
                    for x in left..right {
                        let px = (x as f64 + ox - f64::from(first.x)) / rx;
                        let py = (y as f64 + oy - f64::from(first.y)) / ry;
                        let position = (px * nx + py * ny) / length2;
                        let cap = pen.style & 0xf00;
                        let projection = if cap == 0 {
                            position.clamp(0.0, 1.0)
                        } else {
                            position
                        };
                        let outside = if cap == 0x200 {
                            !(0.0..1.0).contains(&position)
                        } else if cap == 0x100 {
                            position < -1.0 / length2.sqrt()
                                || position > 1.0 + 1.0 / length2.sqrt()
                        } else {
                            false
                        };
                        if !outside
                            && (px - projection * nx).powi(2) + (py - projection * ny).powi(2)
                                <= 1.0
                        {
                            draw(
                                Point32 {
                                    x: x as i32,
                                    y: y as i32,
                                },
                                position.max(0.0) * logical_length,
                            )?;
                        }
                    }
                }
            }
        }
        if damage.left < damage.right && damage.top < damage.bottom {
            self.commit_pixels(memory, index, pixels, damage)?;
        }
        state.current_position = logical_end;
        self.model
            .set_state(args[0], state)
            .expect("line publication retains the exact DC");
        Ok(1)
    }
}

fn dash_on(pen: Pen32, position: f64) -> bool {
    let entries: &[u32] = match pen.style & 15 {
        0 | 6 => return true,
        1 => &[3, 1],
        2 | 8 => &[1, 1],
        3 => &[3, 1, 1, 1],
        4 => &[3, 1, 1, 1, 1, 1],
        5 => return false,
        7 => {
            if let Some(extended) = &pen.extended {
                return user_dash(&extended.entries[..extended.count as usize], position);
            } else {
                return false;
            }
        }
        _ => return false,
    };
    user_dash(entries, position)
}
fn user_dash(entries: &[u32], position: f64) -> bool {
    let total: u64 = entries.iter().map(|&value| u64::from(value)).sum();
    if total == 0 {
        return false;
    }
    let cycles = if entries.len() % 2 == 0 { 1 } else { 2 };
    let mut position = position.floor().max(0.0) as u64 % (total * cycles);
    for i in 0..entries.len() * cycles as usize {
        let length = u64::from(entries[i % entries.len()]);
        if position < length {
            return i % 2 == 0;
        }
        position -= length;
    }
    false
}
