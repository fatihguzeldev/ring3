use super::*;
use crate::process::windows::gdi::CanvasText32;
impl User32Context32 {
    pub(super) fn erase_listview(
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
        let background = self
            .listview(window)
            .map_err(HostError::User32)?
            .background
            .unwrap_or(gdi.system_color(5));
        let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
        let bounds = canvas
            .device_rect(Rect32 {
                left: 0,
                top: 0,
                right: projection.width as i32,
                bottom: projection.height as i32,
            })
            .map_err(HostError::Gdi)?;
        canvas.fill(gdi, bounds, background);
        gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)?;
        Ok(User32Outcome32::Return(1))
    }
    pub(super) fn paint_listview(
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
        let region = if external != 0 {
            Region32::rectangle(bounds)
        } else {
            let (region, _) = self
                .state
                .update_region(window)
                .map_err(HostError::User32)?;
            if region.complexity() == 1 || !self.listview(window).map_err(HostError::User32)?.redraw
            {
                return Ok(User32Outcome32::Return(0));
            }
            self.state.check_paint_change().map_err(HostError::User32)?;
            region
        };
        let font = self.listview_font(window)?;
        let background = self
            .listview(window)
            .map_err(HostError::User32)?
            .background
            .unwrap_or(gdi.system_color(5));
        let own = external == 0;
        let dc = if own {
            gdi.begin_window_paint_dc(projection, region, font, background)
                .map_err(HostError::Gdi)?
        } else {
            external
        };
        let result = (|| {
            let geometry = self.listview_geometry(window, gdi)?;
            let list = self.listview(window).map_err(HostError::User32)?;
            let style = self.state.record(window).map_err(HostError::User32)?.style;
            let focused = self.windows().focus() == Some(window);
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            let clip = canvas.device_rect(bounds).map_err(HostError::Gdi)?;
            canvas
                .with_text_style(gdi, font, false, Some(clip), |canvas| {
                    canvas.fill(gdi, clip, background);
                    for (index, item) in list.items.iter().enumerate() {
                        let row = list.rect(
                            index,
                            geometry.view,
                            geometry.width,
                            geometry.height,
                            geometry.row,
                            geometry.header,
                        );
                        if row.bottom <= geometry.header
                            || row.top >= bounds.bottom
                            || row.right <= 0
                            || row.left >= bounds.right
                        {
                            continue;
                        }
                        let selected = item.state & 2 != 0 && (focused || style & 8 != 0);
                        let selected_background = if focused {
                            gdi.system_color(13)
                        } else {
                            gdi.system_color(15)
                        };
                        let color = if selected && focused {
                            gdi.system_color(14)
                        } else if item.state & 4 != 0 {
                            gdi.system_color(17)
                        } else {
                            list.text_color.unwrap_or(gdi.system_color(8))
                        };
                        let text_background = if selected {
                            None
                        } else {
                            match list.text_background {
                                Some(u32::MAX) => None,
                                Some(color) => Some(color),
                                None => Some(gdi.system_color(5)),
                            }
                        };
                        let columns = if geometry.view == 1 {
                            Some(
                                self.listview_header(window)
                                    .map_err(|_| crate::windows::gdi::GdiError32::InvalidRequest)?,
                            )
                        } else {
                            None
                        };
                        if selected && list.extended & 0x20 != 0 {
                            canvas.fill(gdi, canvas.device_rect(row)?, selected_background);
                        }
                        if let Some(header) = columns {
                            for &column in &header.order {
                                let item_rect = header.rect(column, geometry.header).unwrap();
                                let cell = Rect32 {
                                    left: item_rect.left - list.horizontal,
                                    top: row.top,
                                    right: item_rect.right - list.horizontal,
                                    bottom: row.bottom,
                                };
                                if cell.right <= 0 || cell.left >= bounds.right {
                                    continue;
                                }
                                let sub = self
                                    .listview_column_subitem(window, column)
                                    .map_err(|_| crate::windows::gdi::GdiError32::InvalidRequest)?;
                                if selected && (list.extended & 0x20 != 0 || sub == 0) {
                                    canvas.fill(
                                        gdi,
                                        canvas.device_rect(cell)?,
                                        selected_background,
                                    );
                                }
                                let text = Rect32 {
                                    left: cell.left
                                        + 4
                                        + if sub == 0 {
                                            item.indent.saturating_mul(16)
                                        } else {
                                            0
                                        },
                                    right: cell.right - 4,
                                    ..cell
                                };
                                canvas.with_text_style(
                                    gdi,
                                    font,
                                    false,
                                    Some(canvas.device_rect(cell)?),
                                    |canvas| {
                                        canvas.text(
                                            gdi,
                                            ram,
                                            CanvasText32 {
                                                units: item.text(sub),
                                                bounds: text,
                                                color,
                                                background: text_background,
                                                horizontal: match header.items[column].format & 3 {
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
                                if list.extended & 1 != 0 {
                                    let x = cell.right - 1;
                                    canvas.fill(
                                        gdi,
                                        canvas.device_rect(Rect32 {
                                            left: x,
                                            top: cell.top,
                                            right: x + 1,
                                            bottom: cell.bottom,
                                        })?,
                                        gdi.system_color(15),
                                    );
                                }
                            }
                        } else {
                            if selected {
                                canvas.fill(gdi, canvas.device_rect(row)?, selected_background);
                            }
                            canvas.text(
                                gdi,
                                ram,
                                CanvasText32 {
                                    units: item.text(0),
                                    bounds: Rect32 {
                                        left: row
                                            .left
                                            .saturating_add(4)
                                            .saturating_add(item.indent.saturating_mul(16)),
                                        right: row.right.saturating_sub(4),
                                        ..row
                                    },
                                    color,
                                    background: text_background,
                                    horizontal: if geometry.view == 0 { 1 } else { 0 },
                                    middle: true,
                                    mnemonic: false,
                                    multiline: geometry.view == 0,
                                },
                            )?;
                        }
                        if list.extended & 1 != 0 {
                            canvas.fill(
                                gdi,
                                canvas.device_rect(Rect32 {
                                    left: row.left,
                                    top: row.bottom - 1,
                                    right: row.right,
                                    bottom: row.bottom,
                                })?,
                                gdi.system_color(15),
                            );
                        }
                        if focused && item.state & 1 != 0 {
                            canvas.frame(gdi, canvas.device_rect(row)?, color, color);
                        }
                    }
                    Ok(())
                })
                .map_err(HostError::Gdi)?;
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        if own {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("list-view paint owns its acquired dc")
            );
        }
        result?;
        if own {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("list-view paint was preflighted");
        }
        Ok(User32Outcome32::Return(0))
    }
}
