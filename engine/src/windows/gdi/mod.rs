mod api;
mod bitmap;
mod colors;
mod context;
mod dc;
mod fonts;
mod objects;
mod raster;
mod region;
mod types;

pub use api::{GdiApi32, gdi32_module, user32_gdi_exports};
pub use bitmap::{BitmapLayout32, BitmapPalette32};
pub use context::{GdiContext32, PreparedGdiObject32};
pub use fonts::{
    FontDatabase32, FontMetrics32, GlyphRaster32, LogicalFont32, ShapedGlyph32, ShapedText32,
};
pub use raster::{raster_operation32, raster_uses_source32};
pub use region::Region32;
pub use types::{
    Brush32, DcKind32, DcState32, GdiError32, GdiLimits32, GdiObject32, GdiWindow32, Pen32,
};

#[cfg(test)]
mod tests;
