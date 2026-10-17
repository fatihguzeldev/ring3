mod api;
mod bitmap;
mod colors;
mod context;
mod dc;
mod fonts;
mod objects;
mod palette;
mod raster;
mod region;
mod types;

pub use api::{GdiApi32, gdi32_module, user32_gdi_exports};
pub use bitmap::{BitmapLayout32, BitmapPalette32};
pub(crate) use bitmap::{DibHeader32, decode_dib_header};
pub use context::{GdiContext32, PreparedGdiObject32};
pub use fonts::{
    FontDatabase32, FontMetrics32, GlyphRaster32, LogicalFont32, ShapedGlyph32, ShapedText32,
};
pub use palette::{LogicalPalette32, nearest_palette_index32, synthesized_palette32};
pub use raster::{raster_operation32, raster_uses_source32};
pub use region::Region32;
pub use types::{
    Brush32, DcKind32, DcState32, GdiError32, GdiImageKind32, GdiImageObject32, GdiLimits32,
    GdiObject32, GdiWindow32, Pen32,
};

#[cfg(test)]
mod tests;
