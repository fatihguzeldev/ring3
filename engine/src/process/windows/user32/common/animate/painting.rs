use super::*;
use crate::windows::{gdi::Region32, user32::Rect32};
impl User32Context32 {
    pub(super) fn paint_animation(
        &mut self,
        window: WindowHandle32,
        external: u32,
        message: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        let erase = message == 0x14;
        if matches!(message, 0x14 | 0x318) && external == 0 {
            return Ok(User32Outcome32::Return(0));
        }
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let region = if external != 0 {
            gdi.check_control_erase_dc(external)
                .map_err(HostError::Gdi)?;
            Region32::rectangle(Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            })
        } else {
            let region = self
                .state
                .update_region(window)
                .map_err(HostError::User32)?
                .0;
            if region.complexity() == 1 {
                return Ok(User32Outcome32::Return(0));
            }
            self.state.check_paint_change().map_err(HostError::User32)?;
            region
        };
        let background = gdi.system_color(15);
        let dc = if external != 0 {
            external
        } else {
            gdi.begin_window_paint_dc(projection, region, 0, background)
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
            canvas.fill(
                gdi,
                canvas.device_rect(bounds).map_err(HostError::Gdi)?,
                background,
            );
            if !erase && let Some(clip) = &self.animation(window).map_err(HostError::User32)?.clip {
                let current = self.animation(window).map_err(HostError::User32)?.current;
                let centered = self.state.record(window).map_err(HostError::User32)?.style & 1 != 0;
                let x = if centered {
                    (projection.width as i32 - clip.layout.width as i32) / 2
                } else {
                    0
                };
                let y = if centered {
                    (projection.height as i32 - clip.layout.height as i32) / 2
                } else {
                    0
                };
                for row in 0..clip.layout.height {
                    for column in 0..clip.layout.width {
                        let rect = Rect32 {
                            left: x + column as i32,
                            top: y + row as i32,
                            right: x + column as i32 + 1,
                            bottom: y + row as i32 + 1,
                        };
                        canvas.fill(
                            gdi,
                            canvas.device_rect(rect).map_err(HostError::Gdi)?,
                            clip.color(current, column, row),
                        );
                    }
                }
            }
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if external == 0 {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("animation owns paint dc")
            );
        }
        result?;
        if external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("animation paint was preflighted");
        }
        Ok(User32Outcome32::Return(u32::from(erase)))
    }
}
