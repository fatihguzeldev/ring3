use super::*;
use crate::{
    process::windows::gdi::CanvasText32,
    windows::{
        gdi::{GdiContext32, Region32},
        user32::Rect32,
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(super) fn erase_hotkey_background(
        &self,
        window: WindowHandle32,
        dc: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        if dc == 0 {
            return Ok(User32Outcome32::Return(0));
        }
        let record = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?;
        let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: record.description.extent.width as i32,
            bottom: record.description.extent.height as i32,
        };
        canvas.fill(
            gdi,
            bounds,
            gdi.system_color(if record.description.enabled { 5 } else { 15 }),
        );
        gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)?;
        Ok(User32Outcome32::Return(1))
    }
    pub(super) fn paint_hotkey(
        &mut self,
        window: WindowHandle32,
        external: u32,
        print: bool,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        if print && external == 0 {
            return Err(HostError::User32(E::Win32(6)));
        }
        let (region, _) = self
            .state
            .update_region(window)
            .map_err(HostError::User32)?;
        if external == 0 && region.complexity() == 1 {
            return Ok(User32Outcome32::Return(0));
        }
        if external == 0 {
            self.state.check_paint_change().map_err(HostError::User32)?;
        }
        let State::Hotkey(hotkey) = self.state.common_state(window).map_err(HostError::User32)?
        else {
            return Err(HostError::InvalidRequest);
        };
        let font = if hotkey.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            hotkey.font
        };
        let text = hotkey.label(self.delivered_thread_layout(thread));
        let enabled = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .enabled;
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let background = gdi.system_color(if enabled { 5 } else { 15 });
        let dc = if external != 0 {
            external
        } else {
            gdi.begin_window_paint_dc(projection, region, font, background)
                .map_err(HostError::Gdi)?
        };
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            let bounds = Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            };
            let device = canvas.device_rect(bounds).map_err(HostError::Gdi)?;
            canvas
                .with_text_style(gdi, font, false, Some(device), |canvas| {
                    canvas.fill(gdi, bounds, background);
                    canvas.frame(gdi, bounds, gdi.system_color(16), gdi.system_color(20));
                    canvas.text(
                        gdi,
                        ram,
                        CanvasText32 {
                            units: &text,
                            bounds: Rect32 {
                                left: 3,
                                top: 1,
                                right: bounds.right.saturating_sub(3),
                                bottom: bounds.bottom.saturating_sub(1),
                            },
                            color: gdi.system_color(if enabled { 8 } else { 17 }),
                            background: None,
                            horizontal: 0,
                            middle: true,
                            mnemonic: false,
                            multiline: false,
                        },
                    )
                })
                .map_err(HostError::Gdi)?;
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if external == 0 {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("hotkey owns paint dc")
            );
        }
        result?;
        if external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("hotkey paint preflighted");
        }
        Ok(User32Outcome32::Return(0))
    }
}
