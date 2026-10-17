use super::*;
use crate::process::windows::gdi::{CanvasText32, GdiImageDraw32};
use crate::windows::{
    gdi::{GdiImageKind32, Region32},
    user32::{Point32, Rect32},
};
pub(super) struct StatusPaint32 {
    pub window: WindowHandle32,
    pub external: u32,
    pub print: bool,
    pub call: User32CallContext32,
}
impl User32Context32 {
    pub(super) fn paint_status(
        &mut self,
        request: StatusPaint32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        let StatusPaint32 {
            window,
            external,
            print,
            call,
        } = request;
        if print || external != 0 {
            if external == 0 {
                return Err(HostError::User32(E::Win32(6)));
            }
            gdi.check_control_erase_dc(external)
                .map_err(HostError::Gdi)?;
        } else {
            let (region, _) = self
                .state
                .update_region(window)
                .map_err(HostError::User32)?;
            if region.complexity() == 1 {
                return Ok(User32Outcome32::Return(0));
            }
            self.state.check_paint_change().map_err(HostError::User32)?;
        }
        let status = self.status(window).map_err(HostError::User32)?;
        if !status.simple_mode
            && (0..status.count()).any(|index| status.part(index).unwrap().kind & 0x1000 != 0)
        {
            if external != 0 {
                return Err(HostError::User32(E::Win32(50)));
            }
            return self
                .begin_status_draw(window, 0, true, ram, call)
                .map_err(HostError::User32);
        }
        self.render_status_text_to(window, external, ram, gdi, call.thread_id)?;
        if external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("status paint was preflighted");
        }
        Ok(User32Outcome32::Return(0))
    }
    pub(in crate::process::windows::user32::common::status) fn render_status_text_to(
        &self,
        window: WindowHandle32,
        external: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<(), HostError> {
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
        let dc = if external != 0 {
            external
        } else {
            let (region, _) = self
                .state
                .update_region(window)
                .map_err(HostError::User32)?;
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
            canvas.fill(gdi, device, background);
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
                    continue;
                }
                let device = canvas.device_rect(bounds).map_err(HostError::Gdi)?;
                canvas
                    .with_text_style(gdi, font, part.kind & 0x400 != 0, Some(device), |canvas| {
                        if part.kind & 0x100 == 0 {
                            let (top, bottom) = if part.kind & 0x200 != 0 {
                                (gdi.system_color(20), gdi.system_color(16))
                            } else {
                                (gdi.system_color(16), gdi.system_color(20))
                            };
                            canvas.frame(gdi, device, top, bottom);
                        }
                        let mut content = Rect32 {
                            left: bounds.left.saturating_add(2).min(bounds.right),
                            top: bounds.top.saturating_add(1).min(bounds.bottom),
                            right: bounds.right.saturating_sub(2).max(bounds.left),
                            bottom: bounds.bottom.saturating_sub(1).max(bounds.top),
                        };
                        if part.icon != 0 {
                            let info = gdi.image_info(part.icon)?;
                            if info.kind != GdiImageKind32::Icon {
                                return Err(crate::windows::gdi::GdiError32::InvalidHandle);
                            }
                            let size = (info.width, info.height);
                            canvas.image(
                                gdi,
                                GdiImageDraw32 {
                                    handle: part.icon,
                                    position: Point32 {
                                        x: content.left,
                                        y: content.top
                                            + (content.bottom - content.top - size.1 as i32) / 2,
                                    },
                                    width: size.0,
                                    height: size.1,
                                    flags: 3,
                                    background: 0,
                                },
                            )?;
                            content.left = content
                                .left
                                .saturating_add(size.0 as i32)
                                .saturating_add(2)
                                .min(content.right);
                        }
                        if part.kind & 0x800 != 0 {
                            canvas.text(
                                gdi,
                                ram,
                                CanvasText32 {
                                    units: &part.text,
                                    bounds: content,
                                    color: gdi.system_color(18),
                                    background: None,
                                    horizontal: 0,
                                    middle: true,
                                    mnemonic: false,
                                    multiline: false,
                                },
                            )?;
                        } else {
                            for (alignment, units) in
                                part.text.splitn(3, |unit| *unit == 9).enumerate()
                            {
                                canvas.text(
                                    gdi,
                                    ram,
                                    CanvasText32 {
                                        units,
                                        bounds: content,
                                        color: gdi.system_color(18),
                                        background: None,
                                        horizontal: alignment as u32,
                                        middle: true,
                                        mnemonic: false,
                                        multiline: false,
                                    },
                                )?;
                            }
                        }
                        Ok(())
                    })
                    .map_err(HostError::Gdi)?;
            }
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if external == 0 {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("status owns its acquired dc")
            );
        }
        result
    }
}
