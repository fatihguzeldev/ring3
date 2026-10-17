use super::{
    context::GdiRuntime32,
    storage::{Storage32, SurfaceKey32},
};
use crate::{
    memory::AddressSpace,
    windows::gdi::{Brush32, GdiError32, GdiObject32},
};

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn create_pattern_brush(
        &mut self,
        memory: &AddressSpace,
        bitmap: u32,
    ) -> Result<u32, GdiError32> {
        let GdiObject32::Bitmap(layout) = self.model.object(bitmap)? else {
            return Err(GdiError32::InvalidHandle);
        };
        let source = &self.surfaces[self.surface_index(SurfaceKey32::Bitmap(bitmap))?];
        source.storage.check(memory)?;
        let object = self
            .model
            .prepare_object(GdiObject32::Brush(Brush32::Pattern {
                bitmap,
                monochrome: layout.bits_per_pixel == 1,
            }))?;
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(source.palette.len())
            .map_err(|_| GdiError32::Allocation)?;
        colors.extend_from_slice(&source.palette);
        let pixels = source.storage.read(memory, layout.byte_len()?)?;
        let mut surface =
            self.prepare_surface(SurfaceKey32::PatternBrush(object.handle()), layout, colors)?;
        surface.storage = Storage32::Owned(pixels);
        let handle = self.model.commit_new(object);
        self.publish_surface(surface);
        Ok(handle)
    }
}
