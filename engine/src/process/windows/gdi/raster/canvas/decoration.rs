mod frames;
use super::{GdiCanvas32, intersect};
use crate::{
    process::windows::gdi::GdiRuntime32,
    windows::{
        gdi::GdiError32,
        user32::{Point32, Rect32},
    },
};

impl GdiCanvas32 {
    pub(in crate::process::windows::gdi) fn edge(
        &mut self,
        gdi: &GdiRuntime32,
        mut rect: Rect32,
        edge: u32,
        flags: u32,
    ) -> Result<Rect32, GdiError32> {
        if edge & !15 != 0 || edge & 3 == 3 || edge & 12 == 12 || flags & !0xf81f != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return Ok(rect);
        }
        for (part, inner) in [(edge & 3, false), ((edge >> 2) & 3, true)] {
            if part == 0 {
                continue;
            }
            let (light, dark) = if flags & 0x8000 != 0 {
                (gdi.system_color(5), gdi.system_color(8))
            } else if flags & 0x4000 != 0 {
                (gdi.system_color(16), gdi.system_color(16))
            } else if inner {
                (gdi.system_color(22), gdi.system_color(16))
            } else if flags & 0x1000 != 0 {
                (gdi.system_color(22), gdi.system_color(21))
            } else {
                (gdi.system_color(20), gdi.system_color(21))
            };
            let (top, bottom) = if part == 1 {
                (light, dark)
            } else {
                (dark, light)
            };
            if flags & 0x10 != 0 {
                let rising = flags & 3 != 0 && flags & 12 != 0 && flags & 9 != 9 && flags & 6 != 6;
                let (a, b) = if rising {
                    (
                        Point32 {
                            x: rect.left,
                            y: rect.bottom.saturating_sub(1),
                        },
                        Point32 {
                            x: rect.right.saturating_sub(1),
                            y: rect.top,
                        },
                    )
                } else {
                    (
                        Point32 {
                            x: rect.left,
                            y: rect.top,
                        },
                        Point32 {
                            x: rect.right.saturating_sub(1),
                            y: rect.bottom.saturating_sub(1),
                        },
                    )
                };
                self.decoration_line(gdi, a, b, if flags & 0xa != 0 { top } else { bottom })?;
            } else {
                if flags & 1 != 0 {
                    self.fill(
                        gdi,
                        Rect32 {
                            right: rect.left.saturating_add(1),
                            ..rect
                        },
                        top,
                    );
                }
                if flags & 2 != 0 {
                    self.fill(
                        gdi,
                        Rect32 {
                            bottom: rect.top.saturating_add(1),
                            ..rect
                        },
                        top,
                    );
                }
                if flags & 4 != 0 {
                    self.fill(
                        gdi,
                        Rect32 {
                            left: rect.right.saturating_sub(1),
                            ..rect
                        },
                        bottom,
                    );
                }
                if flags & 8 != 0 {
                    self.fill(
                        gdi,
                        Rect32 {
                            top: rect.bottom.saturating_sub(1),
                            ..rect
                        },
                        bottom,
                    );
                }
            }
            if flags & 1 != 0 {
                rect.left = rect.left.saturating_add(1).min(rect.right);
            }
            if flags & 2 != 0 {
                rect.top = rect.top.saturating_add(1).min(rect.bottom);
            }
            if flags & 4 != 0 {
                rect.right = rect.right.saturating_sub(1).max(rect.left);
            }
            if flags & 8 != 0 {
                rect.bottom = rect.bottom.saturating_sub(1).max(rect.top);
            }
        }
        if flags & 0x800 != 0 && flags & 0x10 == 0 {
            self.fill(
                gdi,
                rect,
                gdi.system_color(if flags & 0x8000 != 0 { 5 } else { 15 }),
            );
        }
        Ok(rect)
    }
    fn decoration_line(
        &mut self,
        gdi: &GdiRuntime32,
        mut first: Point32,
        last: Point32,
        color: u32,
    ) -> Result<(), GdiError32> {
        let dx = (i64::from(last.x) - i64::from(first.x)).abs();
        let dy = (i64::from(last.y) - i64::from(first.y)).abs();
        if dx.max(dy) > 1_048_576 {
            return Err(GdiError32::Capacity);
        }
        let mut error = dx - dy;
        loop {
            self.fill(
                gdi,
                Rect32 {
                    left: first.x,
                    top: first.y,
                    right: first.x.saturating_add(1),
                    bottom: first.y.saturating_add(1),
                },
                color,
            );
            if first == last {
                break;
            }
            let twice = 2 * error;
            if twice > -dy {
                error -= dy;
                first.x += if first.x < last.x { 1 } else { -1 };
            }
            if twice < dx {
                error += dx;
                first.y += if first.y < last.y { 1 } else { -1 };
            }
        }
        Ok(())
    }
    fn decoration_ellipse(&mut self, gdi: &GdiRuntime32, rect: Rect32, color: u32) {
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return;
        }
        let width = f64::from(rect.right) - f64::from(rect.left);
        let height = f64::from(rect.bottom) - f64::from(rect.top);
        let area = intersect(
            rect,
            super::super::super::bitmaps::bounds(self.owner.layout),
        );
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let dx = (f64::from(x) * 2.0 + 1.0 - f64::from(rect.left) - f64::from(rect.right))
                    / width;
                let dy = (f64::from(y) * 2.0 + 1.0 - f64::from(rect.top) - f64::from(rect.bottom))
                    / height;
                if dx * dx + dy * dy <= 1.0 {
                    self.fill(
                        gdi,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 1,
                        },
                        color,
                    );
                }
            }
        }
    }
    fn decoration_triangle(&mut self, gdi: &GdiRuntime32, points: [Point32; 3], color: u32) {
        let edge = |a: Point32, b: Point32, c: Point32| {
            (i128::from(b.x) - i128::from(a.x)) * (i128::from(c.y) - i128::from(a.y))
                - (i128::from(b.y) - i128::from(a.y)) * (i128::from(c.x) - i128::from(a.x))
        };
        let sign = edge(points[0], points[1], points[2]).signum();
        if sign == 0 {
            return;
        }
        let rect = Rect32 {
            left: points.iter().map(|p| p.x).min().unwrap(),
            top: points.iter().map(|p| p.y).min().unwrap(),
            right: points.iter().map(|p| p.x).max().unwrap().saturating_add(1),
            bottom: points.iter().map(|p| p.y).max().unwrap().saturating_add(1),
        };
        let area = intersect(
            rect,
            super::super::super::bitmaps::bounds(self.owner.layout),
        );
        for y in area.top..area.bottom {
            for x in area.left..area.right {
                let p = Point32 { x, y };
                if (0..3).all(|i| edge(points[i], points[(i + 1) % 3], p) * sign >= 0) {
                    self.fill(
                        gdi,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 1,
                        },
                        color,
                    );
                }
            }
        }
    }
}
