use super::*;
use crate::process::windows::gdi::CanvasText32;
use crate::windows::{gdi::Region32, user32::Rect32};

impl User32Context32 {
    pub(in crate::process::windows::user32::common) fn paint_status(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        let (region, _) = self
            .state
            .update_region(window)
            .map_err(HostError::User32)?;
        if region.complexity() == 1 {
            return Ok(User32Outcome32::Return(0));
        }
        self.state.check_paint_change().map_err(HostError::User32)?;
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let status = self.status(window).map_err(HostError::User32)?;
        let font = if status.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            status.font
        };
        let background = if status.background == 0xff000000 {
            gdi.system_color(15)
        } else {
            status.background
        };
        let dc = gdi
            .begin_window_paint_dc(projection, region, font, background)
            .map_err(HostError::Gdi)?;
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            let bounds = Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            };
            canvas.fill(gdi, bounds, background);
            let count = if status.simple_mode {
                1
            } else {
                status.count()
            };
            for index in 0..count {
                let (part, bounds) = if status.simple_mode {
                    (
                        status.text_part(0xff).unwrap(),
                        Rect32 {
                            top: 2.min(bounds.bottom),
                            ..bounds
                        },
                    )
                } else {
                    (
                        status.part(index).unwrap(),
                        status.rect(index, bounds.right, bounds.bottom).unwrap(),
                    )
                };
                if part.kind & 0x1000 != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                if part.kind & 0x100 == 0 {
                    let (top, bottom) = if part.kind & 0x200 != 0 {
                        (gdi.system_color(20), gdi.system_color(16))
                    } else {
                        (gdi.system_color(16), gdi.system_color(20))
                    };
                    canvas.frame(gdi, bounds, top, bottom);
                }
                let bounds = Rect32 {
                    left: bounds.left.saturating_add(2).min(bounds.right),
                    top: bounds.top.saturating_add(1).min(bounds.bottom),
                    right: bounds.right.saturating_sub(2).max(bounds.left),
                    bottom: bounds.bottom.saturating_sub(1).max(bounds.top),
                };
                if part.kind & 0x800 != 0 {
                    canvas
                        .text(
                            gdi,
                            ram,
                            CanvasText32 {
                                units: &part.text,
                                bounds,
                                color: gdi.system_color(18),
                                background: None,
                                horizontal: 0,
                                middle: true,
                                mnemonic: false,
                                multiline: false,
                            },
                        )
                        .map_err(HostError::Gdi)?;
                } else {
                    for (alignment, units) in part.text.splitn(3, |unit| *unit == 9).enumerate() {
                        canvas
                            .text(
                                gdi,
                                ram,
                                CanvasText32 {
                                    units,
                                    bounds,
                                    color: gdi.system_color(18),
                                    background: None,
                                    horizontal: alignment as u32,
                                    middle: true,
                                    mnemonic: false,
                                    multiline: false,
                                },
                            )
                            .map_err(HostError::Gdi)?;
                    }
                }
            }
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("status owns its paint dc")
        );
        result?;
        self.state
            .set_update_region(window, Region32::default(), false)
            .expect("status paint was preflighted");
        Ok(User32Outcome32::Return(0))
    }
}
