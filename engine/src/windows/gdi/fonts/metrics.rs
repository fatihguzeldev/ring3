use super::{super::GdiError32, FontDatabase32, FontMetrics32, LogicalFont32};
impl FontDatabase32 {
    pub(super) fn scale(&self, font: &LogicalFont32) -> Result<(f32, f32), GdiError32> {
        font.check()?;
        let face = &self.face(font).design;
        let units = face.units;
        let height = if font.height == 0 { 16 } else { font.height };
        let pixel_em = if height < 0 {
            (-height) as f32
        } else {
            height as f32 * units / (face.ascent + face.descent)
        };
        let natural_average = face.average;
        let x_scale = if font.width == 0 {
            1.0
        } else {
            font.width as f32 / (natural_average * pixel_em / units)
        };
        Ok((pixel_em, x_scale))
    }
    pub fn metrics(&self, font: &LogicalFont32) -> Result<FontMetrics32, GdiError32> {
        let (pixels, x_scale) = self.scale(font)?;
        let face = &self.face(font).design;
        let scale = pixels / face.units;
        let ascent = (face.ascent * scale).round() as i32;
        let descent = (face.descent * scale).round() as i32;
        Ok(FontMetrics32 {
            height: ascent + descent,
            ascent,
            descent,
            internal_leading: (ascent + descent - pixels.round() as i32).max(0),
            external_leading: (face.line_gap * scale).round() as i32,
            average_width: (face.average * scale * x_scale).round() as i32,
            maximum_width: (face.maximum * scale * x_scale).ceil() as i32,
            weight: if font.weight == 0 { 400 } else { font.weight },
            first: face.first,
            last: face.last,
            default: 0x3f,
            break_character: 0x20,
            fixed_pitch: face.monospace,
            italic: font.italic || face.italic,
            underline: font.underline,
            strike_out: font.strike_out,
            charset: font.charset,
        })
    }
    pub fn advance(&self, font: &LogicalFont32, glyph: u16) -> Result<f32, GdiError32> {
        if glyph >= self.glyph_count(font) {
            return Err(GdiError32::InvalidRequest);
        }
        let (pixels, x_scale) = self.scale(font)?;
        Ok(self
            .face(font)
            .raster
            .metrics_indexed(glyph, pixels)
            .advance_width
            * x_scale)
    }
}
