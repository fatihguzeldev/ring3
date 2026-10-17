use super::{
    FontDatabase32, FontMetrics32, GlyphRaster32, LogicalFont32, ShapedText32, faces::FontFace32,
};
use crate::windows::gdi::GdiError32;
use std::sync::Arc;

#[derive(Clone)]
pub struct ResolvedFont32 {
    pub(super) descriptor: LogicalFont32,
    pub(super) face: Arc<FontFace32>,
}
impl core::fmt::Debug for ResolvedFont32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResolvedFont32")
            .field("descriptor", &self.descriptor)
            .field("face", &self.face.name)
            .finish()
    }
}
impl PartialEq for ResolvedFont32 {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && Arc::ptr_eq(&self.face, &other.face)
    }
}
impl Eq for ResolvedFont32 {}
impl FontDatabase32 {
    pub fn resolve(&self, font: LogicalFont32) -> Result<ResolvedFont32, GdiError32> {
        font.check()?;
        Ok(ResolvedFont32 {
            face: self.face_arc(&font).clone(),
            descriptor: font,
        })
    }
    pub fn metrics(&self, font: &LogicalFont32) -> Result<FontMetrics32, GdiError32> {
        self.face(font).metrics(font)
    }
    pub fn advance(&self, font: &LogicalFont32, glyph: u16) -> Result<f32, GdiError32> {
        self.face(font).advance(font, glyph)
    }
    pub fn shape(
        &self,
        font: &LogicalFont32,
        units: &[u16],
        indices: bool,
        rtl: bool,
    ) -> Result<ShapedText32, GdiError32> {
        self.face(font).shape(font, units, indices, rtl)
    }
    pub fn rasterize(
        &self,
        font: &LogicalFont32,
        glyph: u16,
        budget: usize,
    ) -> Result<GlyphRaster32, GdiError32> {
        self.face(font).rasterize(font, glyph, budget)
    }
}
impl ResolvedFont32 {
    pub(super) fn from_face(descriptor: LogicalFont32, face: Arc<FontFace32>) -> Self {
        Self { descriptor, face }
    }
    pub fn descriptor(&self) -> &LogicalFont32 {
        &self.descriptor
    }
    pub fn face_name(&self) -> &[u16] {
        &self.face.name
    }
    pub fn glyph_index(&self, character: char) -> u16 {
        self.face.raster.lookup_glyph_index(character)
    }
    pub fn glyph_count(&self) -> u16 {
        self.face.raster.glyph_count()
    }
    pub fn metrics(&self) -> Result<FontMetrics32, GdiError32> {
        self.face.metrics(&self.descriptor)
    }
    pub fn advance(&self, glyph: u16) -> Result<f32, GdiError32> {
        self.face.advance(&self.descriptor, glyph)
    }
    pub fn shape(
        &self,
        units: &[u16],
        indices: bool,
        rtl: bool,
    ) -> Result<ShapedText32, GdiError32> {
        self.face.shape(&self.descriptor, units, indices, rtl)
    }
    pub fn rasterize(&self, glyph: u16, budget: usize) -> Result<GlyphRaster32, GdiError32> {
        self.face.rasterize(&self.descriptor, glyph, budget)
    }
}
