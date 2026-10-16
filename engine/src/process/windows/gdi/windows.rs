use super::{
    context::GdiRuntime32,
    storage::{Storage32, SurfaceKey32},
};
use crate::windows::gdi::{BitmapLayout32, DcKind32, GdiError32};

pub(in crate::process) use crate::windows::gdi::GdiWindow32;
impl GdiRuntime32 {
    pub(super) fn memory_dc(&mut self, source: u32, thread: u32) -> Result<u32, GdiError32> {
        if source != 0 {
            self.model.dc_kind(source)?;
        }
        let prepared = self.model.prepare_dc(DcKind32::Memory {
            owner: (source == 0).then_some(thread),
        })?;
        let surface = self.prepare_surface(
            SurfaceKey32::Default(prepared.handle()),
            BitmapLayout32::new(1, 1, 1, false)?,
            vec![0, 0x00ff_ffff],
        )?;
        let handle = self.model.commit_new(prepared);
        self.publish_surface(surface);
        Ok(handle)
    }
    pub(super) fn acquire_dc(
        &mut self,
        window: GdiWindow32,
        thread: u32,
    ) -> Result<u32, GdiError32> {
        let key = SurfaceKey32::Window {
            window: window.window,
            whole: window.whole,
        };
        let class = (!window.whole && window.class_style & 0x40 != 0).then_some(window.class_atom);
        let private = !window.whole && window.class_style & 0x20 != 0;
        let persistent = self.model.persistent_dc(window.window, class, window.whole);
        let prepared = if persistent.is_none() {
            Some(self.model.prepare_dc(DcKind32::Display {
                window: window.window,
                thread,
                private,
                class,
                window_dc: window.whole,
            })?)
        } else {
            None
        };
        let layout = BitmapLayout32::new(
            window.width.max(1) as i32,
            window.height.max(1) as i32,
            32,
            false,
        )?;
        if let Ok(index) = self.surface_index(key) {
            if self.surfaces[index].layout != layout {
                let length = layout.byte_len()?;
                let old = self.surfaces[index].layout.byte_len()?;
                if length > self.config.bitmap_bytes - self.bytes + old {
                    return Err(GdiError32::Capacity);
                }
                let mut pixels = super::memory::zeroes(length)?;
                let Storage32::Owned(previous) = &self.surfaces[index].storage else {
                    return Err(GdiError32::InvalidRequest);
                };
                let old_layout = self.surfaces[index].layout;
                for y in 0..old_layout.height.min(layout.height) {
                    let count = old_layout.width.min(layout.width) as usize * 4;
                    pixels[layout.row(y)..layout.row(y) + count]
                        .copy_from_slice(&previous[old_layout.row(y)..old_layout.row(y) + count]);
                }
                let revision = self.surfaces[index]
                    .revision
                    .checked_add(1)
                    .ok_or(GdiError32::Exhausted)?;
                self.surfaces[index].storage = Storage32::Owned(pixels);
                self.surfaces[index].layout = layout;
                self.surfaces[index].revision = revision;
                self.surfaces[index].damage = super::bitmaps::bounds(layout);
                self.bytes = self.bytes - old + length;
            }
        } else {
            let surface = self.prepare_surface(key, layout, Vec::new())?;
            self.publish_surface(surface);
        }
        if let Some(prepared) = prepared {
            Ok(self.model.commit_new(prepared))
        } else {
            let handle = persistent.unwrap();
            self.model.bind_display_dc(handle, window.window, thread)?;
            Ok(handle)
        }
    }
    pub(super) fn screen_dc(&mut self) -> Result<u32, GdiError32> {
        let prepared = self.model.prepare_dc(DcKind32::Screen)?;
        let key = SurfaceKey32::Window {
            window: 0,
            whole: false,
        };
        if self.surface_index(key).is_err() {
            let surface = self.prepare_surface(
                key,
                BitmapLayout32::new(
                    self.config.screen_width as i32,
                    self.config.screen_height as i32,
                    32,
                    false,
                )?,
                Vec::new(),
            )?;
            self.publish_surface(surface);
        }
        Ok(self.model.commit_new(prepared))
    }
    pub(in crate::process) fn retire_dead_windows(&mut self, mut alive: impl FnMut(u32) -> bool) {
        let dead: Vec<u32> = self
            .surfaces
            .iter()
            .filter_map(|surface| match surface.key {
                SurfaceKey32::Window { window, .. } if window != 0 && !alive(window) => {
                    Some(window)
                }
                _ => None,
            })
            .collect();
        for window in dead {
            self.model.retire_window(window);
        }
        let mut removed = 0;
        self.surfaces.retain(|surface| { let keep = !matches!(surface.key, SurfaceKey32::Window { window, .. } if window != 0 && !alive(window)); if !keep { removed += surface.layout.byte_len().unwrap(); } keep });
        self.bytes -= removed;
        self.prune_defaults();
    }
}
