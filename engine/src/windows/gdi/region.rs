use super::GdiError32;
use crate::windows::user32::{Point32, Rect32};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region32 {
    rects: Vec<Rect32>,
}
impl Region32 {
    pub fn rectangle(rect: Rect32) -> Self {
        Self {
            rects: if rect.left < rect.right && rect.top < rect.bottom {
                vec![rect]
            } else {
                Vec::new()
            },
        }
    }
    pub fn rects(&self) -> &[Rect32] {
        &self.rects
    }
    pub fn contains(&self, point: Point32) -> bool {
        self.rects.iter().any(|r| {
            point.x >= r.left && point.x < r.right && point.y >= r.top && point.y < r.bottom
        })
    }
    pub fn bounds(&self) -> Rect32 {
        self.rects
            .iter()
            .copied()
            .reduce(|a, b| Rect32 {
                left: a.left.min(b.left),
                top: a.top.min(b.top),
                right: a.right.max(b.right),
                bottom: a.bottom.max(b.bottom),
            })
            .unwrap_or_default()
    }
    pub fn complexity(&self) -> u32 {
        match self.rects.len() {
            0 => 1,
            1 => 2,
            _ => 3,
        }
    }
    pub fn combine(&self, other: &Self, mode: u32) -> Result<Self, GdiError32> {
        if mode == 5 {
            return Ok(self.clone());
        }
        if !(1..=4).contains(&mode) {
            return Err(GdiError32::InvalidRequest);
        }
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for rect in self.rects.iter().chain(&other.rects) {
            xs.extend([rect.left, rect.right]);
            ys.extend([rect.top, rect.bottom]);
        }
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();
        if xs.len().saturating_mul(ys.len()) > 65_536 {
            return Err(GdiError32::Capacity);
        }
        let mut rects: Vec<Rect32> = Vec::new();
        rects
            .try_reserve(4096.min(xs.len().saturating_mul(ys.len())))
            .map_err(|_| GdiError32::Allocation)?;
        for y in ys.windows(2) {
            let mut run: Option<Rect32> = None;
            for x in xs.windows(2) {
                let p = Point32 { x: x[0], y: y[0] };
                let a = self.contains(p);
                let b = other.contains(p);
                let include = match mode {
                    1 => a && b,
                    2 => a || b,
                    3 => a != b,
                    4 => a && !b,
                    _ => unreachable!(),
                };
                if include {
                    if let Some(rect) = run.as_mut() {
                        rect.right = x[1];
                    } else {
                        run = Some(Rect32 {
                            left: x[0],
                            right: x[1],
                            top: y[0],
                            bottom: y[1],
                        });
                    }
                } else if let Some(rect) = run.take() {
                    append(&mut rects, rect)?;
                }
            }
            if let Some(rect) = run {
                append(&mut rects, rect)?;
            }
        }
        rects.sort_unstable_by_key(|rect| (rect.top, rect.left, rect.bottom, rect.right));
        Ok(Self { rects })
    }
    pub fn offset(&self, dx: i32, dy: i32) -> Result<Self, GdiError32> {
        let mut next = self.clone();
        for r in &mut next.rects {
            r.left = r.left.checked_add(dx).ok_or(GdiError32::InvalidRequest)?;
            r.right = r.right.checked_add(dx).ok_or(GdiError32::InvalidRequest)?;
            r.top = r.top.checked_add(dy).ok_or(GdiError32::InvalidRequest)?;
            r.bottom = r.bottom.checked_add(dy).ok_or(GdiError32::InvalidRequest)?;
        }
        Ok(next)
    }
}
fn append(rects: &mut Vec<Rect32>, rect: Rect32) -> Result<(), GdiError32> {
    if let Some(previous) = rects.iter_mut().rev().find(|previous| {
        previous.bottom == rect.top && previous.left == rect.left && previous.right == rect.right
    }) {
        previous.bottom = rect.bottom;
    } else {
        if rects.len() == 4096 {
            return Err(GdiError32::Capacity);
        }
        rects.push(rect);
    }
    Ok(())
}
