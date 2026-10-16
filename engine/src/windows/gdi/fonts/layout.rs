use super::{
    super::GdiError32, FontDatabase32, GlyphRaster32, LogicalFont32, ShapedGlyph32, ShapedText32,
};
impl FontDatabase32 {
    pub fn shape(
        &self,
        font: &LogicalFont32,
        units: &[u16],
        glyph_indices: bool,
        right_to_left: bool,
    ) -> Result<ShapedText32, GdiError32> {
        if units.len() > 8192 {
            return Err(GdiError32::Capacity);
        }
        let metrics = self.metrics(font)?;
        let mut glyphs = Vec::new();
        glyphs
            .try_reserve(units.len())
            .map_err(|_| GdiError32::Allocation)?;
        let mut advance = 0.0;
        if glyph_indices {
            for (cluster, glyph) in units.iter().copied().enumerate() {
                let width = self.advance(font, glyph)?;
                glyphs.push(ShapedGlyph32 {
                    glyph,
                    cluster,
                    x: advance,
                    y: 0.0,
                    advance: width,
                });
                advance += width;
            }
        } else {
            let text = String::from_utf16_lossy(units);
            let mut clusters = Vec::new();
            clusters
                .try_reserve(text.len() + 1)
                .map_err(|_| GdiError32::Allocation)?;
            let mut unit = 0;
            for (byte, character) in text.char_indices() {
                clusters.push((byte, unit));
                unit += character.len_utf16();
            }
            let (pixels, x_scale) = self.scale(font)?;
            let face = rustybuzz::Face::from_slice(&self.face(font).bytes, 0)
                .ok_or(GdiError32::InvalidRequest)?;
            let scale = pixels / face.units_per_em() as f32;
            let bidi = unicode_bidi::BidiInfo::new(
                &text,
                Some(if right_to_left {
                    unicode_bidi::Level::rtl()
                } else {
                    unicode_bidi::Level::ltr()
                }),
            );
            for paragraph in &bidi.paragraphs {
                let (levels, runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
                for run in runs {
                    let mut input = rustybuzz::UnicodeBuffer::new();
                    input.push_str(&text[run.clone()]);
                    input.set_direction(if levels[run.start].is_rtl() {
                        rustybuzz::Direction::RightToLeft
                    } else {
                        rustybuzz::Direction::LeftToRight
                    });
                    input.guess_segment_properties();
                    let shaped = rustybuzz::shape(&face, &[], input);
                    if glyphs.len() + shaped.len() > 32_768 {
                        return Err(GdiError32::Capacity);
                    }
                    glyphs
                        .try_reserve(shaped.len())
                        .map_err(|_| GdiError32::Allocation)?;
                    for (info, position) in
                        shaped.glyph_infos().iter().zip(shaped.glyph_positions())
                    {
                        let byte = run.start + info.cluster as usize;
                        let cluster = clusters
                            .binary_search_by_key(&byte, |entry| entry.0)
                            .ok()
                            .map_or(0, |index| clusters[index].1);
                        let width = position.x_advance as f32 * scale * x_scale;
                        glyphs.push(ShapedGlyph32 {
                            glyph: info.glyph_id as u16,
                            cluster,
                            x: advance + position.x_offset as f32 * scale * x_scale,
                            y: -(position.y_offset as f32 * scale),
                            advance: width,
                        });
                        advance += width;
                    }
                }
            }
        }
        Ok(ShapedText32 {
            glyphs,
            advance,
            metrics,
        })
    }
    pub fn rasterize(
        &self,
        font: &LogicalFont32,
        glyph: u16,
        budget: usize,
    ) -> Result<GlyphRaster32, GdiError32> {
        if glyph >= self.glyph_count(font) {
            return Err(GdiError32::InvalidRequest);
        }
        let (pixels, scale) = self.scale(font)?;
        let source = self.face(font).raster.metrics_indexed(glyph, pixels);
        let bold = if font.weight >= 600 {
            (pixels / 32.0).ceil() as usize
        } else {
            0
        };
        let width = (source.width as f32 * scale).ceil() as usize;
        let skew = if font.italic {
            (source.height as f32 * 0.2).ceil() as usize
        } else {
            0
        };
        let width = width
            .checked_add(bold)
            .and_then(|width| width.checked_add(skew))
            .ok_or(GdiError32::Capacity)?;
        let size = width
            .checked_mul(source.height)
            .ok_or(GdiError32::Capacity)?;
        if size > budget
            || source
                .width
                .checked_mul(source.height)
                .is_none_or(|size| size > budget)
        {
            return Err(GdiError32::Capacity);
        }
        let (_, coverage) = self.face(font).raster.rasterize_indexed(glyph, pixels);
        let mut result = Vec::new();
        result
            .try_reserve_exact(size)
            .map_err(|_| GdiError32::Allocation)?;
        result.resize(size, 0);
        let natural_width = (source.width as f32 * scale).ceil() as usize;
        for y in 0..source.height {
            let offset = if font.italic {
                ((source.height - 1 - y) as f32 * 0.2).round() as usize
            } else {
                0
            };
            for x in 0..natural_width {
                let value = coverage[y * source.width
                    + ((x as f32 / scale) as usize).min(source.width.saturating_sub(1))];
                for stroke in 0..=bold {
                    let index = y * width + x + offset + stroke;
                    if let Some(pixel) = result.get_mut(index) {
                        *pixel = (*pixel).max(value);
                    }
                }
            }
        }
        Ok(GlyphRaster32 {
            left: (source.xmin as f32 * scale).round() as i32,
            top: -source.ymin - source.height as i32,
            width,
            height: source.height,
            coverage: result,
        })
    }
}
