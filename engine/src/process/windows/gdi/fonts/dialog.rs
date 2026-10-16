use super::super::context::GdiRuntime32;
use crate::windows::{
    gdi::{GdiContext32, GdiError32, GdiObject32, LogicalFont32},
    user32::Point32,
};

impl GdiRuntime32 {
    pub(in crate::process) fn system_font(&self) -> Result<LogicalFont32, GdiError32> {
        self.logical_font(GdiContext32::stock_object(13)?)
    }
    pub(in crate::process) fn dialog_base_units(
        &self,
        font: &LogicalFont32,
    ) -> Result<Point32, GdiError32> {
        let alphabet: [u16; 52] = std::array::from_fn(|index| {
            if index < 26 {
                u16::from(b'A') + index as u16
            } else {
                u16::from(b'a') + (index - 26) as u16
            }
        });
        let shaped = self.fonts.shape(font, &alphabet, false, false)?;
        if !shaped.advance.is_finite() || shaped.advance < 0.0 || shaped.advance > i32::MAX as f32 {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(Point32 {
            x: (shaped.advance.round() as i32 / 26 + 1) / 2,
            y: shaped.metrics.height,
        })
    }
    pub(in crate::process) fn retire_dialog_font(&mut self, font: u32) -> Result<(), GdiError32> {
        if !matches!(self.model.object(font)?, GdiObject32::Font(_)) {
            return Err(GdiError32::InvalidHandle);
        }
        self.model.delete_object(font)
    }
}
