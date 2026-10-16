use super::super::GdiError32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalFont32 {
    pub height: i32,
    pub width: i32,
    pub escapement: i32,
    pub orientation: i32,
    pub weight: i32,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub charset: u8,
    pub out_precision: u8,
    pub clip_precision: u8,
    pub quality: u8,
    pub pitch_family: u8,
    pub face: Vec<u16>,
}
impl Default for LogicalFont32 {
    fn default() -> Self {
        Self {
            height: 16,
            width: 0,
            escapement: 0,
            orientation: 0,
            weight: 400,
            italic: false,
            underline: false,
            strike_out: false,
            charset: 1,
            out_precision: 0,
            clip_precision: 0,
            quality: 0,
            pitch_family: 0,
            face: Vec::new(),
        }
    }
}
impl LogicalFont32 {
    pub fn check(&self) -> Result<(), GdiError32> {
        if self.height == i32::MIN
            || self.height.abs() > 4096
            || self.width < 0
            || self.width > 4096
            || !(0..=1000).contains(&self.weight)
            || self.face.len() >= 32
            || self.face.contains(&0)
        {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
    pub fn stock(index: u32) -> Self {
        Self {
            pitch_family: u8::from(matches!(index, 10 | 11 | 16)),
            ..Default::default()
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontMetrics32 {
    pub height: i32,
    pub ascent: i32,
    pub descent: i32,
    pub internal_leading: i32,
    pub external_leading: i32,
    pub average_width: i32,
    pub maximum_width: i32,
    pub weight: i32,
    pub first: u16,
    pub last: u16,
    pub default: u16,
    pub break_character: u16,
    pub fixed_pitch: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub charset: u8,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph32 {
    pub glyph: u16,
    pub cluster: usize,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
}
pub struct ShapedText32 {
    pub glyphs: Vec<ShapedGlyph32>,
    pub advance: f32,
    pub metrics: FontMetrics32,
}
pub struct GlyphRaster32 {
    pub left: i32,
    pub top: i32,
    pub width: usize,
    pub height: usize,
    pub coverage: Vec<u8>,
}
