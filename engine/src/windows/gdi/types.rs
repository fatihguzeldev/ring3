use crate::windows::user32::{Point32, Rect32};

#[derive(Clone, Copy)]
pub struct GdiWindow32 {
    pub window: u32,
    pub thread: u32,
    pub class_atom: u16,
    pub class_style: u32,
    pub background_brush: u32,
    pub width: u32,
    pub height: u32,
    pub whole: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdiError32 {
    InvalidConfig,
    InvalidRequest,
    InvalidHandle,
    SelectedObject,
    Unsupported,
    Capacity,
    Allocation,
    Exhausted,
    Memory(crate::memory::MemoryError),
}
impl GdiError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidHandle => 6,
            Self::InvalidConfig | Self::InvalidRequest | Self::SelectedObject => 87,
            Self::Unsupported => 50,
            Self::Capacity | Self::Allocation | Self::Exhausted => 8,
            Self::Memory(_) => 998,
        }
    }
}
impl From<crate::memory::MemoryError> for GdiError32 {
    fn from(value: crate::memory::MemoryError) -> Self {
        Self::Memory(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GdiLimits32 {
    pub objects: usize,
    pub dcs: usize,
    pub saved_states: usize,
}
impl Default for GdiLimits32 {
    fn default() -> Self {
        Self {
            objects: 4096,
            dcs: 1024,
            saved_states: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush32 {
    Solid(u32),
    System(u32),
    Null,
    Hatch { style: u32, color: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pen32 {
    pub style: u32,
    pub width: i32,
    pub color: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GdiObject32 {
    Brush(Brush32),
    Pen(Pen32),
    Bitmap(super::BitmapLayout32),
    Region(super::Region32),
    StockFont(u32),
    Font(super::LogicalFont32),
    StockPalette,
}
impl GdiObject32 {
    pub fn object_type(&self) -> u32 {
        match self {
            Self::Pen(_) => 1,
            Self::Brush(_) => 2,
            Self::StockPalette => 5,
            Self::StockFont(_) | Self::Font(_) => 6,
            Self::Bitmap(_) => 7,
            Self::Region(_) => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcKind32 {
    Memory {
        owner: Option<u32>,
    },
    Display {
        window: u32,
        thread: u32,
        private: bool,
        class: Option<u16>,
        window_dc: bool,
    },
    Screen,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DcState32 {
    pub bitmap: u32,
    pub brush: u32,
    pub pen: u32,
    pub font: u32,
    pub palette: u32,
    pub text_color: u32,
    pub text_align: u32,
    pub character_extra: i32,
    pub justification_extra: i32,
    pub justification_breaks: u32,
    pub background_color: u32,
    pub background_mode: u32,
    pub dc_brush_color: u32,
    pub dc_pen_color: u32,
    pub rop2: u32,
    pub map_mode: u32,
    pub stretch_mode: u32,
    pub window_origin: Point32,
    pub viewport_origin: Point32,
    pub window_extent: Point32,
    pub viewport_extent: Point32,
    pub brush_origin: Point32,
    pub current_position: Point32,
    pub clip: Option<super::Region32>,
}
impl Default for DcState32 {
    fn default() -> Self {
        Self {
            bitmap: super::context::DEFAULT_BITMAP,
            brush: super::context::stock_handle(0),
            pen: super::context::stock_handle(7),
            font: super::context::stock_handle(13),
            palette: super::context::stock_handle(15),
            text_color: 0,
            text_align: 0,
            character_extra: 0,
            justification_extra: 0,
            justification_breaks: 0,
            background_color: 0x00ff_ffff,
            background_mode: 2,
            dc_brush_color: 0x00ff_ffff,
            dc_pen_color: 0,
            rop2: 13,
            map_mode: 1,
            stretch_mode: 1,
            window_origin: Point32::default(),
            viewport_origin: Point32::default(),
            window_extent: Point32 { x: 1, y: 1 },
            viewport_extent: Point32 { x: 1, y: 1 },
            brush_origin: Point32::default(),
            current_position: Point32::default(),
            clip: None,
        }
    }
}
impl DcState32 {
    pub fn device_point(&self, point: Point32) -> Result<Point32, GdiError32> {
        mapped_point(
            point,
            self.window_origin,
            self.viewport_origin,
            self.window_extent,
            self.viewport_extent,
        )
    }
    pub fn logical_point(&self, point: Point32) -> Result<Point32, GdiError32> {
        mapped_point(
            point,
            self.viewport_origin,
            self.window_origin,
            self.viewport_extent,
            self.window_extent,
        )
    }
    pub fn accepts(&self, point: Point32, bounds: Rect32) -> bool {
        point.x >= bounds.left
            && point.x < bounds.right
            && point.y >= bounds.top
            && point.y < bounds.bottom
            && self.clip.as_ref().is_none_or(|clip| clip.contains(point))
    }
}

fn mapped_point(
    point: Point32,
    source: Point32,
    target: Point32,
    source_extent: Point32,
    target_extent: Point32,
) -> Result<Point32, GdiError32> {
    let axis = |value: i32, window: i32, viewport: i32, numerator: i32, denominator: i32| {
        if denominator == 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let product = (i64::from(value) - i64::from(window)) * i64::from(numerator);
        let divisor = i64::from(denominator);
        let quotient = product / divisor;
        let remainder = product % divisor;
        let rounded = quotient
            + if remainder.abs() * 2 >= divisor.abs() {
                if (product < 0) != (divisor < 0) {
                    -1
                } else {
                    1
                }
            } else {
                0
            };
        i32::try_from(rounded + i64::from(viewport)).map_err(|_| GdiError32::InvalidRequest)
    };
    Ok(Point32 {
        x: axis(
            point.x,
            source.x,
            target.x,
            target_extent.x,
            source_extent.x,
        )?,
        y: axis(
            point.y,
            source.y,
            target.y,
            target_extent.y,
            source_extent.y,
        )?,
    })
}
