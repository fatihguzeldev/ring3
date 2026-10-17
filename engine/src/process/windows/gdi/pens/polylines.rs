use super::super::{bitmaps::bounds, context::GdiRuntime32, memory, raster::binary_rop};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiError32, GdiObject32},
        user32::{Point32, Rect32},
    },
};

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn polyline_to(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let mut state = self.model.state(args[0])?.clone();
        if args[2] == 0 || args[2] > 65_536 {
            return Err(GdiError32::InvalidRequest);
        }
        let GdiObject32::Pen(pen) = self.model.object(state.pen)? else {
            return Err(GdiError32::InvalidHandle);
        };
        if !matches!(pen.style & 15, 0 | 5 | 6) || pen.width > 1 || pen.style & 0x10000 != 0 {
            return Err(GdiError32::Unsupported);
        }
        let mut bytes = memory::zeroes(args[2] as usize * 8)?;
        memory.read(GuestAddress(args[1]), &mut bytes)?;
        let points = || {
            bytes.chunks_exact(8).map(|point| Point32 {
                x: i32::from_le_bytes(point[..4].try_into().unwrap()),
                y: i32::from_le_bytes(point[4..].try_into().unwrap()),
            })
        };
        let mut previous = state.device_point(state.current_position)?;
        let mut work = 0u64;
        for point in points() {
            let point = state.device_point(point)?;
            let length = (i64::from(point.x) - i64::from(previous.x))
                .abs()
                .max((i64::from(point.y) - i64::from(previous.y)).abs())
                as u64;
            work += length;
            if work > 1_048_576 {
                return Err(GdiError32::Capacity);
            }
            previous = point;
        }
        let index = self.dc_surface(args[0])?;
        let surface = &self.surfaces[index];
        let layout = surface.layout;
        let mut pixels = surface.storage.read(memory, layout.byte_len()?)?;
        let mut damage: Option<Rect32> = None;
        for logical_end in points() {
            let mut point = state.device_point(state.current_position)?;
            let end = state.device_point(logical_end)?;
            let dx = (i64::from(end.x) - i64::from(point.x)).abs();
            let dy = (i64::from(end.y) - i64::from(point.y)).abs();
            let sx = if point.x < end.x { 1 } else { -1 };
            let sy = if point.y < end.y { 1 } else { -1 };
            let mut error = dx - dy;
            while pen.style & 15 != 5 && point != end {
                if state.accepts(point, bounds(layout))
                    && let Some(color) = self.pen_color(&state, state.pen, pen, point)?
                {
                    let old = layout.read_pixel(
                        &pixels,
                        &surface.palette,
                        point.x as u32,
                        point.y as u32,
                    );
                    layout.write_pixel(
                        &mut pixels,
                        &surface.palette,
                        point.x as u32,
                        point.y as u32,
                        binary_rop(state.rop2, color, old),
                    );
                    damage = Some(match damage {
                        Some(rect) => Rect32 {
                            left: rect.left.min(point.x),
                            top: rect.top.min(point.y),
                            right: rect.right.max(point.x + 1),
                            bottom: rect.bottom.max(point.y + 1),
                        },
                        None => Rect32 {
                            left: point.x,
                            top: point.y,
                            right: point.x + 1,
                            bottom: point.y + 1,
                        },
                    });
                }
                let twice = error * 2;
                if twice > -dy {
                    error -= dy;
                    point.x += sx;
                }
                if twice < dx {
                    error += dx;
                    point.y += sy;
                }
            }
            state.current_position = logical_end;
        }
        if let Some(damage) = damage {
            self.commit_pixels(memory, index, pixels, damage)?;
        }
        self.model.set_state(args[0], state)?;
        Ok(1)
    }
}
