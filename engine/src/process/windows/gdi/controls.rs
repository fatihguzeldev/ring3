mod canvas;
mod widgets;
use super::{context::GdiRuntime32, windows::GdiWindow32};
use crate::{
    memory::AddressSpace,
    windows::{
        controls::{ControlKind32, ControlState32},
        gdi::{DcKind32, GdiContext32, GdiError32, Region32},
        user32::Rect32,
    },
};
use canvas::Canvas32;

pub(in crate::process) struct ControlVisual32<'a> {
    pub kind: ControlKind32,
    pub style: u32,
    pub enabled: bool,
    pub focused: bool,
    pub text: &'a [u16],
    pub control: &'a ControlState32,
}
impl GdiRuntime32 {
    pub(in crate::process) fn check_control_visual(
        &self,
        visual: &ControlVisual32<'_>,
    ) -> Result<(), GdiError32> {
        match visual.kind {
            ControlKind32::Button
                if (visual.style & 15 >= 8 && visual.style & 15 != 9)
                    || visual.style & (0x40 | 0x80) != 0 =>
            {
                return Err(GdiError32::Unsupported);
            }
            ControlKind32::Static
                if matches!(visual.style & 31, 3 | 10 | 13 | 14 | 15)
                    || visual.style & 31 > 18
                    || visual.style & 0xc000 != 0 =>
            {
                return Err(GdiError32::Unsupported);
            }
            ControlKind32::ListBox
                if visual.style & (8 | 0x10 | 0x20 | 0x80 | 0x200 | 0x800 | 0x2000) != 0 =>
            {
                return Err(GdiError32::Unsupported);
            }
            ControlKind32::ComboBox if visual.style & (0x10 | 0x20) != 0 => {
                return Err(GdiError32::Unsupported);
            }
            ControlKind32::ScrollBar if visual.style & 0x18 != 0 => {
                return Err(GdiError32::Unsupported);
            }
            _ => {}
        }
        self.logical_font(if visual.control.font == 0 {
            GdiContext32::stock_object(13)?
        } else {
            visual.control.font
        })?;
        Ok(())
    }
    pub(in crate::process) fn begin_control_dc(
        &mut self,
        window: GdiWindow32,
        region: Region32,
        font: u32,
        kind: ControlKind32,
    ) -> Result<u32, GdiError32> {
        if window.class_style & (0x20 | 0x40) != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let font = if font == 0 {
            GdiContext32::stock_object(13)?
        } else {
            font
        };
        self.logical_font(font)?;
        let dc = self.acquire_dc(window, window.thread)?;
        let mut state = self.model.state(dc).expect("accepted control DC").clone();
        state.font = font;
        state.clip = Some(region);
        state.background_color = if matches!(
            kind,
            ControlKind32::Edit | ControlKind32::ListBox | ControlKind32::ComboBox
        ) {
            self.system_color(5)
        } else {
            self.system_color(15)
        };
        self.model
            .set_state(dc, state)
            .expect("accepted control DC");
        Ok(dc)
    }
    pub(in crate::process) fn check_control_dc(
        &self,
        dc: u32,
        window: u32,
        thread: u32,
    ) -> Result<(), GdiError32> {
        if !matches!(self.model.dc_kind(dc)?, DcKind32::Display { window: owner, thread: caller, private: false, class: None, window_dc: false } if owner == window && caller == thread)
        {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
    pub(in crate::process) fn release_control_dc(
        &mut self,
        dc: u32,
        window: u32,
        thread: u32,
    ) -> Result<(), GdiError32> {
        self.check_control_dc(dc, window, thread)?;
        assert!(self.model.release_dc(window, dc, thread)?);
        Ok(())
    }
    pub(in crate::process) fn render_control(
        &mut self,
        memory: &mut AddressSpace,
        dc: u32,
        visual: &ControlVisual32<'_>,
        brush: u32,
    ) -> Result<(), GdiError32> {
        self.check_control_visual(visual)?;
        let mut canvas = Canvas32::new(self, memory, dc)?;
        let index = self.dc_surface(dc)?;
        let bounds = super::bitmaps::bounds(self.surfaces[index].layout);
        let push_button = visual.kind == ControlKind32::Button
            && (matches!(visual.style & 15, 0 | 1) || visual.style & 0x1000 != 0);
        if push_button {
            canvas.set_background_color(self.system_color(15));
        }
        if brush == 0 || push_button {
            canvas.fill(
                self,
                bounds,
                if matches!(
                    visual.kind,
                    ControlKind32::Edit | ControlKind32::ListBox | ControlKind32::ComboBox
                ) {
                    self.system_color(5)
                } else {
                    self.system_color(15)
                },
            );
        } else {
            canvas.background(self, bounds, brush)?;
        }
        widgets::paint(self, memory, &mut canvas, dc, visual, bounds)?;
        let pixels = canvas.finish();
        if pixels.damage.left < pixels.damage.right && pixels.damage.top < pixels.damage.bottom {
            self.commit_pixels(memory, pixels.index, pixels.pixels, pixels.damage)?;
        }
        Ok(())
    }
    pub(in crate::process) fn check_control_erase_dc(&self, dc: u32) -> Result<(), GdiError32> {
        self.dc_surface(dc)?;
        Ok(())
    }
    pub(in crate::process) fn erase_control(
        &mut self,
        memory: &mut AddressSpace,
        dc: u32,
        kind: ControlKind32,
        bounds: Rect32,
        brush: u32,
    ) -> Result<(), GdiError32> {
        let mut canvas = Canvas32::new(self, memory, dc)?;
        if brush != 0 {
            canvas.background(self, bounds, brush)?;
        } else {
            canvas.fill(
                self,
                bounds,
                if matches!(
                    kind,
                    ControlKind32::Edit | ControlKind32::ListBox | ControlKind32::ComboBox
                ) {
                    self.system_color(5)
                } else {
                    self.system_color(15)
                },
            );
        }
        let pixels = canvas.finish();
        if pixels.damage.left < pixels.damage.right && pixels.damage.top < pixels.damage.bottom {
            self.commit_pixels(memory, pixels.index, pixels.pixels, pixels.damage)?;
        }
        Ok(())
    }
}
