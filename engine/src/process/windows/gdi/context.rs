use super::{GdiConfig32, storage::Surface32};
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange},
    windows::gdi::{GdiContext32, GdiError32, GdiLimits32},
};

pub(in crate::process) struct GdiRuntime32 {
    pub(super) config: GdiConfig32,
    pub(super) model: GdiContext32,
    pub(super) colors: crate::windows::user32::SystemColors32,
    pub(super) bounds: PageRange,
    pub(super) surfaces: Vec<Surface32>,
    pub(super) next_surface: u64,
    pub(super) bytes: usize,
    pub(super) fonts: crate::windows::gdi::FontDatabase32,
    pub(super) paints: Vec<super::paint::PaintLease32>,
}
impl GdiRuntime32 {
    pub(in crate::process) fn new(config: GdiConfig32) -> Result<Self, GdiError32> {
        if config.bitmap_base == 0
            || config.bitmap_bytes == 0
            || config.bitmap_bytes > u32::MAX as usize
            || config.screen_width == 0
            || config.screen_height == 0
            || config.screen_width > 16_384
            || config.screen_height > 16_384
            || u64::from(config.screen_width) * u64::from(config.screen_height) * 4
                > config.bitmap_bytes as u64
        {
            return Err(GdiError32::InvalidConfig);
        }
        let bounds = PageRange::new(GuestAddress(config.bitmap_base), config.bitmap_pages)
            .map_err(|_| GdiError32::InvalidConfig)?;
        let model = GdiContext32::new(GdiLimits32 {
            objects: config.objects,
            dcs: config.dcs,
            saved_states: config.saved_states,
        })?;
        let mut surfaces = Vec::new();
        surfaces
            .try_reserve_exact(config.objects + config.dcs)
            .map_err(|_| GdiError32::Allocation)?;
        let mut paints = Vec::new();
        paints
            .try_reserve_exact(config.dcs)
            .map_err(|_| GdiError32::Allocation)?;
        Ok(Self {
            config,
            model,
            colors: Default::default(),
            bounds,
            surfaces,
            next_surface: 1,
            bytes: 0,
            paints,
            fonts: crate::windows::gdi::FontDatabase32::new(16 << 20)?,
        })
    }
    pub(in crate::process) fn config(&self) -> GdiConfig32 {
        self.config
    }
    pub(in crate::process) fn guest_reservations(&self) -> impl Iterator<Item = PageRange> {
        core::iter::once(self.bounds)
    }
    pub(in crate::process) fn process_exit_ranges(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.surfaces
            .iter()
            .filter_map(|surface| surface.storage.range())
    }
    pub(in crate::process) fn check_process_exit(
        &self,
        memory: &AddressSpace,
    ) -> Result<(), GdiError32> {
        for surface in &self.surfaces {
            surface.storage.check(memory)?;
        }
        Ok(())
    }
    pub(in crate::process) fn retire_process(&mut self) {
        self.model.clear();
        self.surfaces.clear();
        self.paints.clear();
        self.bytes = 0;
    }
    pub(in crate::process) fn retire_thread(&mut self, thread: u32) {
        self.model.retire_thread(thread);
        self.paints.retain(|paint| paint.thread != thread);
        self.prune_defaults();
    }
    pub(super) fn prune_defaults(&mut self) {
        let model = &self.model;
        let mut removed = 0;
        self.surfaces.retain(|surface| { let keep = !matches!(surface.key, super::storage::SurfaceKey32::Default(dc) if model.dc_kind(dc).is_err()); if !keep { removed += surface.layout.byte_len().unwrap(); } keep });
        self.bytes -= removed;
    }
}
