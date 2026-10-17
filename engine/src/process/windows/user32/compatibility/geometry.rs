use super::super::memory;
use super::*;
use crate::{
    memory::GuestAddress,
    windows::{
        user32::{Point32, Rect32},
        window::WindowHandle32,
    },
};

fn empty(r: Rect32) -> bool {
    r.left >= r.right || r.top >= r.bottom
}
fn origin(this: &User32Context32, window: u32) -> Result<Point32, User32Error32> {
    if window == 0 {
        Ok(Point32::default())
    } else {
        Ok(this
            .state
            .record(WindowHandle32::try_from(window)?)?
            .client_origin)
    }
}
impl User32Context32 {
    pub(super) fn geometry_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        m: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        if api == MapWindowPoints {
            if a[3] > 65536 {
                return Err(User32Error32::Win32(87));
            }
            let from = origin(self, a[0])?;
            let to = origin(self, a[1])?;
            let dx = from.x.wrapping_sub(to.x);
            let dy = from.y.wrapping_sub(to.y);
            let mut bytes = Vec::with_capacity(a[3] as usize * 8);
            for i in 0..a[3] {
                let at = a[2].checked_add(i * 8).ok_or(User32Error32::Win32(87))?;
                let [x, y] = memory::words(m, at)?;
                bytes.extend_from_slice(&(x as i32).wrapping_add(dx).to_le_bytes());
                bytes.extend_from_slice(&(y as i32).wrapping_add(dy).to_le_bytes());
            }
            m.write(GuestAddress(a[2]), &bytes)?;
            return Ok(User32Outcome32::Return(
                (dx as u32 & 0xffff) | ((dy as u32 & 0xffff) << 16),
            ));
        }
        let mut r = match api {
            SetRect => Rect32 {
                left: a[1] as i32,
                top: a[2] as i32,
                right: a[3] as i32,
                bottom: a[4] as i32,
            },
            SetRectEmpty => Rect32::default(),
            CopyRect | IntersectRect | UnionRect => memory::rect(m, a[1])?,
            _ => memory::rect(m, a[0])?,
        };
        let mut result = 1;
        match api {
            EqualRect => {
                return Ok(User32Outcome32::Return(u32::from(
                    r == memory::rect(m, a[1])?,
                )));
            }
            IsRectEmpty => return Ok(User32Outcome32::Return(u32::from(empty(r)))),
            PtInRect => {
                return Ok(User32Outcome32::Return(u32::from(
                    (a[1] as i32) >= r.left
                        && (a[1] as i32) < r.right
                        && (a[2] as i32) >= r.top
                        && (a[2] as i32) < r.bottom,
                )));
            }
            InflateRect => {
                r.left = r.left.wrapping_sub(a[1] as i32);
                r.right = r.right.wrapping_add(a[1] as i32);
                r.top = r.top.wrapping_sub(a[2] as i32);
                r.bottom = r.bottom.wrapping_add(a[2] as i32);
            }
            OffsetRect => {
                r.left = r.left.wrapping_add(a[1] as i32);
                r.right = r.right.wrapping_add(a[1] as i32);
                r.top = r.top.wrapping_add(a[2] as i32);
                r.bottom = r.bottom.wrapping_add(a[2] as i32);
            }
            IntersectRect | UnionRect => {
                let other = memory::rect(m, a[2])?;
                r = if api == IntersectRect {
                    Rect32 {
                        left: r.left.max(other.left),
                        top: r.top.max(other.top),
                        right: r.right.min(other.right),
                        bottom: r.bottom.min(other.bottom),
                    }
                } else if empty(r) {
                    other
                } else if empty(other) {
                    r
                } else {
                    Rect32 {
                        left: r.left.min(other.left),
                        top: r.top.min(other.top),
                        right: r.right.max(other.right),
                        bottom: r.bottom.max(other.bottom),
                    }
                };
                if empty(r) {
                    r = Rect32::default();
                    result = 0;
                }
            }
            AdjustWindowRectEx => {
                // nonclient extents match the engine's default window procedure.
                let border = if a[1] & 0x0004_0000 != 0 {
                    8
                } else if a[1] & 0x0080_0000 != 0 {
                    1
                } else {
                    0
                };
                let caption = if a[1] & 0x00c0_0000 == 0x00c0_0000 {
                    23
                } else {
                    0
                };
                r.left = r.left.wrapping_sub(border);
                r.right = r
                    .right
                    .wrapping_add(border + if a[1] & 0x0020_0000 != 0 { 17 } else { 0 });
                r.top = r
                    .top
                    .wrapping_sub(border + caption + if a[2] != 0 { 20 } else { 0 });
                r.bottom = r
                    .bottom
                    .wrapping_add(border + if a[1] & 0x0010_0000 != 0 { 17 } else { 0 });
                if a[3] & 0x200 != 0 {
                    r.left -= 2;
                    r.top -= 2;
                    r.right += 2;
                    r.bottom += 2;
                }
            }
            _ => {}
        }
        memory::write_rect(m, a[0], r)?;
        Ok(User32Outcome32::Return(result))
    }
}
