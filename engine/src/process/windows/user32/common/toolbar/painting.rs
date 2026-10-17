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
    pub(super) fn erase_toolbar(
        &self,
        window: WindowHandle32,
        dc: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        if dc == 0 {
            return Ok(User32Outcome32::Return(0));
        }
        gdi.check_control_erase_dc(dc).map_err(HostError::Gdi)?;
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
        let bounds = canvas.device_rect(bounds).map_err(HostError::Gdi)?;
        canvas.fill(gdi, bounds, gdi.system_color(15));
        gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)?;
        Ok(User32Outcome32::Return(1))
    }
    pub(super) fn paint_toolbar(
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
        if external != 0 {
            gdi.check_control_erase_dc(external)
                .map_err(HostError::Gdi)?;
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
        let State::Toolbar(toolbar) = self.state.common_state(window).map_err(HostError::User32)?
        else {
            return Err(HostError::InvalidRequest);
        };
        let font = if toolbar.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            toolbar.font
        };
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
        let background = gdi.system_color(15);
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
                    canvas.fill(gdi, device, background);
                    for (index, rect) in toolbar.layout() {
                        let button = &toolbar.buttons[index];
                        if button.style & 1 != 0 {
                            let line = Rect32 {
                                left: rect.left + rect.right.saturating_sub(rect.left) / 2,
                                top: 3,
                                right: rect.left + rect.right.saturating_sub(rect.left) / 2 + 1,
                                bottom: rect.bottom.saturating_sub(3),
                            };
                            let line = canvas.device_rect(line)?;
                            canvas.fill(gdi, line, gdi.system_color(16));
                            continue;
                        }
                        let device = canvas.device_rect(rect)?;
                        let down = button.state & 3 != 0;
                        if down {
                            canvas.fill(gdi, device, gdi.system_color(22));
                        }
                        canvas.frame(
                            gdi,
                            device,
                            gdi.system_color(if down { 16 } else { 20 }),
                            gdi.system_color(if down { 20 } else { 16 }),
                        );
                        let shift = i32::from(down);
                        canvas.text(
                            gdi,
                            ram,
                            CanvasText32 {
                                units: &button.text,
                                bounds: Rect32 {
                                    left: rect.left + 3 + shift,
                                    top: rect.top + 1 + shift,
                                    right: rect.right.saturating_sub(3),
                                    bottom: rect.bottom.saturating_sub(1),
                                },
                                color: gdi.system_color(
                                    if enabled && button.state & 4 != 0 && button.state & 0x10 == 0
                                    {
                                        18
                                    } else {
                                        17
                                    },
                                ),
                                background: None,
                                horizontal: 1,
                                middle: true,
                                mnemonic: button.style & 0x20 == 0,
                                multiline: false,
                            },
                        )?;
                    }
                    Ok(())
                })
                .map_err(HostError::Gdi)?;
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if external == 0 {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("toolbar owns paint dc")
            );
        }
        result?;
        if external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("toolbar paint preflighted");
        }
        Ok(User32Outcome32::Return(0))
    }
}
