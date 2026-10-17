use super::*;
use crate::{process::windows::gdi::CanvasText32, windows::gdi::GdiContext32};
impl User32Context32 {
    fn header_font(&self, window: WindowHandle32) -> Result<u32, HostError> {
        let font = self.header(window).map_err(HostError::User32)?.font;
        if font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)
        } else {
            Ok(font)
        }
    }
    pub(super) fn header_geometry(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
    ) -> Result<u32, HostError> {
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        match args[1] {
            0x1205 => {
                let [rect_pointer, pos_pointer] =
                    memory::words::<2>(ram, args[3]).map_err(HostError::User32)?;
                let mut rect = memory::rect(ram, rect_pointer).map_err(HostError::User32)?;
                let hidden = self.state.record(window).map_err(HostError::User32)?.style & 8 != 0;
                let height = if hidden {
                    0
                } else {
                    gdi.font_metrics(self.header_font(window)?)
                        .map_err(HostError::Gdi)?
                        .height
                        .saturating_add(2)
                        .max(0)
                };
                let width = rect.right.saturating_sub(rect.left).max(0);
                let position = [
                    window.get(),
                    0,
                    rect.left as u32,
                    rect.top as u32,
                    width as u32,
                    height as u32,
                    0x14,
                ];
                let bytes = position
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                rect.top = rect
                    .top
                    .checked_add(height)
                    .ok_or(HostError::User32(E::Win32(87)))?;
                let rect_bytes = [rect.left, rect.top, rect.right, rect.bottom]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>();
                ram.write_spans(&[
                    (GuestAddress(pos_pointer), &bytes),
                    (GuestAddress(rect_pointer), &rect_bytes),
                ])
                .map_err(HostError::Memory)?;
                Ok(1)
            }
            0x1206 => {
                let point = memory::words::<2>(ram, args[3]).map_err(HostError::User32)?;
                let (index, flags) = self.header(window).map_err(HostError::User32)?.hit(
                    Point32 {
                        x: point[0] as i32,
                        y: point[1] as i32,
                    },
                    projection.width as i32,
                    projection.height as i32,
                );
                let output = [flags, index as u32]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                ram.write(
                    GuestAddress(
                        args[3]
                            .checked_add(8)
                            .ok_or(HostError::User32(E::Win32(87)))?,
                    ),
                    &output,
                )
                .map_err(HostError::Memory)?;
                Ok(index as u32)
            }
            0x1207 => {
                let Some(rect) = self
                    .header(window)
                    .map_err(HostError::User32)?
                    .rect(args[2] as usize, projection.height as i32)
                else {
                    return Ok(0);
                };
                memory::write_rect(ram, args[3], rect).map_err(HostError::User32)?;
                Ok(1)
            }
            _ => Err(HostError::InvalidRequest),
        }
    }
    pub(super) fn erase_header(
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
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
        let rect = canvas
            .device_rect(Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            })
            .map_err(HostError::Gdi)?;
        canvas.fill(gdi, rect, gdi.system_color(15));
        gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)?;
        Ok(User32Outcome32::Return(1))
    }
    pub(super) fn paint_header(
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
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: projection.width as i32,
            bottom: projection.height as i32,
        };
        let region = if print || external != 0 {
            Region32::rectangle(bounds)
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
        let font = self.header_font(window)?;
        let own = external == 0;
        let dc = if own {
            gdi.begin_window_paint_dc(projection, region, font, gdi.system_color(15))
                .map_err(HostError::Gdi)?
        } else {
            external
        };
        let result = (|| {
            let header = self.header(window).map_err(HostError::User32)?;
            let style = self.state.record(window).map_err(HostError::User32)?.style;
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            canvas
                .with_text_style(
                    gdi,
                    font,
                    false,
                    Some(canvas.device_rect(bounds).map_err(HostError::Gdi)?),
                    |canvas| {
                        canvas.fill(gdi, canvas.device_rect(bounds)?, gdi.system_color(15));
                        for &index in &header.order {
                            let rect = header
                                .rect(index, bounds.bottom)
                                .expect("same header order");
                            if rect.right == rect.left {
                                continue;
                            }
                            let pressed = header.track.is_some_and(|track| {
                                !track.divider && track.key == header.items[index].key
                            });
                            if style & 2 != 0 {
                                canvas.frame(
                                    gdi,
                                    canvas.device_rect(rect)?,
                                    gdi.system_color(if pressed { 16 } else { 20 }),
                                    gdi.system_color(if pressed { 20 } else { 16 }),
                                );
                            } else {
                                canvas.fill(
                                    gdi,
                                    canvas.device_rect(Rect32 {
                                        left: rect.right - 1,
                                        top: rect.top,
                                        right: rect.right,
                                        bottom: rect.bottom,
                                    })?,
                                    gdi.system_color(16),
                                );
                            }
                            let item = &header.items[index];
                            if item.format & 0x4000 != 0 {
                                let mut text = Rect32 {
                                    left: rect.left.saturating_add(4 + i32::from(pressed)),
                                    top: rect.top + 1,
                                    right: rect.right.saturating_sub(4),
                                    bottom: rect.bottom - 1,
                                };
                                if item.format & 0x600 != 0 {
                                    text.right = text.right.saturating_sub(10);
                                }
                                canvas.with_text_style(
                                    gdi,
                                    font,
                                    item.format & 4 != 0,
                                    Some(canvas.device_rect(rect)?),
                                    |canvas| {
                                        canvas.text(
                                            gdi,
                                            ram,
                                            CanvasText32 {
                                                units: &item.text,
                                                bounds: text,
                                                color: gdi.system_color(18),
                                                background: None,
                                                horizontal: match item.format & 3 {
                                                    1 => 2,
                                                    2 => 1,
                                                    _ => 0,
                                                },
                                                middle: true,
                                                mnemonic: false,
                                                multiline: false,
                                            },
                                        )
                                    },
                                )?;
                            }
                            if item.format & 0x600 != 0 {
                                let center = rect.right - 7;
                                let top = (rect.top + rect.bottom) / 2 - 2;
                                for row in 0..4 {
                                    let half = if item.format & 0x400 != 0 {
                                        row
                                    } else {
                                        3 - row
                                    };
                                    canvas.fill(
                                        gdi,
                                        canvas.device_rect(Rect32 {
                                            left: center - half,
                                            top: top + row,
                                            right: center + half + 1,
                                            bottom: top + row + 1,
                                        })?,
                                        gdi.system_color(18),
                                    );
                                }
                            }
                        }
                        if let Some(track) = header
                            .track
                            .filter(|track| track.divider && style & 0x80 == 0)
                            && let Some(index) =
                                header.items.iter().position(|item| item.key == track.key)
                        {
                            let rect = header
                                .rect(index, bounds.bottom)
                                .expect("same tracked header item");
                            let x = rect.left.saturating_add(track.proposed);
                            canvas.fill(
                                gdi,
                                canvas.device_rect(Rect32 {
                                    left: x,
                                    top: 0,
                                    right: x.saturating_add(1),
                                    bottom: bounds.bottom,
                                })?,
                                gdi.system_color(18),
                            );
                        }
                        Ok(())
                    },
                )
                .map_err(HostError::Gdi)?;
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if own {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("header paint owns its acquired dc")
            );
        }
        result?;
        if !print && external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("header paint was preflighted");
        }
        Ok(User32Outcome32::Return(0))
    }
}
