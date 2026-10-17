use crate::windows::{
    gdi::{DcState32, GdiError32},
    user32::{Point32, Rect32},
};

#[derive(Clone, Copy)]
pub(super) struct TextRotation32 {
    cosine: f64,
    sine: f64,
    origin: Point32,
}
impl TextRotation32 {
    pub(super) fn new(escapement: i32, origin: Point32) -> Self {
        let angle = escapement.rem_euclid(3600);
        let (cosine, sine) = match angle {
            0 => (1.0, 0.0),
            900 => (0.0, 1.0),
            1800 => (-1.0, 0.0),
            2700 => (0.0, -1.0),
            _ => {
                let radians = f64::from(angle) * core::f64::consts::PI / 1800.0;
                (radians.cos(), radians.sin())
            }
        };
        Self {
            cosine,
            sine,
            origin,
        }
    }
    pub(super) fn identity() -> Self {
        Self::new(0, Point32::default())
    }
    fn unrotated(self) -> bool {
        self.cosine == 1.0 && self.sine == 0.0
    }
    fn logical(self, x: f64, y: f64) -> (f64, f64) {
        let dx = x - f64::from(self.origin.x);
        let dy = y - f64::from(self.origin.y);
        (
            f64::from(self.origin.x) + self.cosine * dx + self.sine * dy,
            f64::from(self.origin.y) - self.sine * dx + self.cosine * dy,
        )
    }
    pub(super) fn device_bounds(
        self,
        state: &DcState32,
        rectangle: Rect32,
    ) -> Result<Rect32, GdiError32> {
        if self.unrotated() {
            return super::rendering::device_rect(state, rectangle);
        }
        let mut left = f64::INFINITY;
        let mut top = f64::INFINITY;
        let mut right = f64::NEG_INFINITY;
        let mut bottom = f64::NEG_INFINITY;
        for (x, y) in [
            (rectangle.left, rectangle.top),
            (rectangle.right, rectangle.top),
            (rectangle.left, rectangle.bottom),
            (rectangle.right, rectangle.bottom),
        ] {
            let (x, y) = self.logical(f64::from(x), f64::from(y));
            let x = map(
                x,
                state.window_origin.x,
                state.viewport_origin.x,
                state.window_extent.x,
                state.viewport_extent.x,
            )?;
            let y = map(
                y,
                state.window_origin.y,
                state.viewport_origin.y,
                state.window_extent.y,
                state.viewport_extent.y,
            )?;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
        Ok(Rect32 {
            left: coordinate(left.floor())?,
            top: coordinate(top.floor())?,
            right: coordinate(right.ceil())?,
            bottom: coordinate(bottom.ceil())?,
        })
    }
    pub(super) fn accepts(
        self,
        state: &DcState32,
        rectangle: Rect32,
        point: Point32,
    ) -> Result<bool, GdiError32> {
        if self.unrotated() {
            return Ok(true);
        }
        let x = map(
            f64::from(point.x) + 0.5,
            state.viewport_origin.x,
            state.window_origin.x,
            state.viewport_extent.x,
            state.window_extent.x,
        )?;
        let y = map(
            f64::from(point.y) + 0.5,
            state.viewport_origin.y,
            state.window_origin.y,
            state.viewport_extent.y,
            state.window_extent.y,
        )?;
        let dx = x - f64::from(self.origin.x);
        let dy = y - f64::from(self.origin.y);
        let x = f64::from(self.origin.x) + self.cosine * dx - self.sine * dy;
        let y = f64::from(self.origin.y) + self.sine * dx + self.cosine * dy;
        Ok(x >= f64::from(rectangle.left)
            && x < f64::from(rectangle.right)
            && y >= f64::from(rectangle.top)
            && y < f64::from(rectangle.bottom))
    }
    pub(super) fn advance(self, advance: f32) -> Result<Point32, GdiError32> {
        let (x, y) = self.logical(
            f64::from(self.origin.x) + f64::from(advance),
            f64::from(self.origin.y),
        );
        Ok(Point32 {
            x: coordinate(x.round())?,
            y: coordinate(y.round())?,
        })
    }
}
fn coordinate(value: f64) -> Result<i32, GdiError32> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(GdiError32::InvalidRequest);
    }
    Ok(value as i32)
}
fn map(
    value: f64,
    from: i32,
    to: i32,
    from_extent: i32,
    to_extent: i32,
) -> Result<f64, GdiError32> {
    if from_extent == 0 || to_extent == 0 {
        return Err(GdiError32::InvalidRequest);
    }
    let result =
        f64::from(to) + (value - f64::from(from)) * f64::from(to_extent) / f64::from(from_extent);
    if !result.is_finite() {
        return Err(GdiError32::InvalidRequest);
    }
    Ok(result)
}
