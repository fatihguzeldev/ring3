use super::context::GdiRuntime32;

impl GdiRuntime32 {
    pub(super) fn nearest_color(
        &mut self,
        dc: u32,
        color: u32,
    ) -> Result<u32, crate::windows::gdi::GdiError32> {
        use crate::windows::gdi::{BitmapLayout32, GdiError32};
        let index = self.dc_surface(dc)?;
        let surface = &self.surfaces[index];
        let color = if color >> 24 == 1 {
            *self
                .selected_palette(dc)?
                .get((color & 0xffff) as usize)
                .ok_or(GdiError32::InvalidRequest)?
        } else {
            color & 0x00ff_ffff
        };
        let mut pixel_layout = BitmapLayout32::new(1, 1, surface.layout.bits_per_pixel, false)?;
        pixel_layout.masks = surface.layout.masks;
        let mut pixel = [0; 4];
        pixel_layout.write_pixel(&mut pixel, &surface.palette, 0, 0, color);
        Ok(pixel_layout.read_pixel(&pixel, &surface.palette, 0, 0))
    }
    pub(in crate::process) fn system_color(&self, index: u32) -> u32 {
        self.colors.get(index).unwrap_or(0)
    }
    pub(in crate::process) fn system_colors(&self) -> crate::windows::user32::SystemColors32 {
        self.colors
    }
    pub(in crate::process) fn set_system_colors(
        &mut self,
        colors: crate::windows::user32::SystemColors32,
    ) {
        self.colors = colors;
    }
}
