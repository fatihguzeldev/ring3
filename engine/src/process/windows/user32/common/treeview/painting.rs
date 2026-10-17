use super::*;
use crate::process::windows::gdi::CanvasText32;
use crate::windows::gdi::GdiError32;
struct Row32 {
    id: u32,
    whole: Rect32,
    label: Rect32,
    button: Rect32,
}
impl User32Context32 {
    fn tree_font(&self, window: WindowHandle32) -> Result<u32, E> {
        let value = self.tree(window)?.font;
        Ok(if value == 0 {
            GdiContext32::stock_object(17).map_err(|_| E::Win32(6))?
        } else {
            value
        })
    }
    pub(super) fn tree_row_height(
        &self,
        window: WindowHandle32,
        gdi: &GdiRuntime32,
    ) -> Result<i32, GdiError32> {
        let tree = self.tree(window).map_err(|_| GdiError32::InvalidRequest)?;
        let font = self
            .tree_font(window)
            .map_err(|_| GdiError32::InvalidRequest)?;
        let metrics = gdi.font_metrics(font)?;
        let height = if tree.item_height == 0 {
            metrics.height.saturating_add(2).max(2)
        } else {
            tree.item_height
        };
        let style = self
            .state
            .record(window)
            .map_err(|_| GdiError32::InvalidRequest)?
            .style;
        Ok(if style & 0x4000 == 0 {
            height & !1
        } else {
            height
        }
        .max(1))
    }
    fn tree_rows(
        &self,
        window: WindowHandle32,
        gdi: &GdiRuntime32,
    ) -> Result<Vec<Row32>, HostError> {
        let tree = self.tree(window).map_err(HostError::User32)?;
        let height = self.tree_row_height(window, gdi).map_err(HostError::Gdi)?;
        let bounds = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        let style = self.state.record(window).map_err(HostError::User32)?.style;
        let ids = tree.visible_ids().map_err(HostError::User32)?;
        let start = ids.iter().position(|&id| id == tree.first).unwrap_or(0);
        let mut rows = Vec::new();
        rows.try_reserve_exact(ids.len().saturating_sub(start))
            .map_err(|_| HostError::User32(E::Allocation))?;
        for (index, &id) in ids[start..].iter().enumerate() {
            let top = (index as i32).saturating_mul(height);
            if top >= bounds.height as i32 {
                break;
            }
            let depth = tree.depth(id) as i32 + i32::from(style & 4 != 0);
            let x = depth.saturating_mul(tree.indent);
            rows.push(Row32 {
                id,
                whole: Rect32 {
                    left: 0,
                    top,
                    right: bounds.width as i32,
                    bottom: top.saturating_add(height),
                },
                label: Rect32 {
                    left: x.saturating_add(tree.indent),
                    top,
                    right: bounds.width as i32,
                    bottom: top.saturating_add(height),
                },
                button: Rect32 {
                    left: x + (tree.indent - 9) / 2,
                    top: top + (height - 9) / 2,
                    right: x + (tree.indent - 9) / 2 + 9,
                    bottom: top + (height - 9) / 2 + 9,
                },
            });
        }
        Ok(rows)
    }
    pub(super) fn tree_geometry_message(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        _thread: u32,
    ) -> Result<u32, HostError> {
        let rows = self.tree_rows(window, gdi)?;
        if args[1] == 0x1111 {
            let [x, y, _, _] = memory::words::<4>(ram, args[3]).map_err(HostError::User32)?;
            let point = Point32 {
                x: x as i32,
                y: y as i32,
            };
            let (id, flags) =
                rows.iter()
                    .find(|row| inside(row.whole, point))
                    .map_or((0, 1), |row| {
                        (
                            row.id,
                            if inside(row.button, point)
                                && self.tree(window).expect("same tree").has_children(row.id)
                                && self.state.record(window).expect("same tree").style & 1 != 0
                            {
                                0x10
                            } else if point.x < row.label.left {
                                8
                            } else {
                                4
                            },
                        )
                    });
            let bytes = [x, y, flags, id]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            ram.write(GuestAddress(args[3]), &bytes)
                .map_err(E::from)
                .map_err(HostError::User32)?;
            return Ok(id);
        }
        if args[1] == 0x1110 {
            return Ok(rows.len() as u32);
        }
        let id = memory::words::<1>(ram, args[3]).map_err(HostError::User32)?[0];
        let Some(row) = rows.iter().find(|row| row.id == id) else {
            return Ok(0);
        };
        let rect = if args[2] != 0 {
            self.tree_label_rect(window, row, ram, gdi)?
        } else {
            row.whole
        };
        let bytes = [rect.left, rect.top, rect.right, rect.bottom]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        ram.write(GuestAddress(args[3]), &bytes)
            .map_err(E::from)
            .map_err(HostError::User32)?;
        Ok(1)
    }
    fn tree_label_rect(
        &self,
        window: WindowHandle32,
        row: &Row32,
        ram: &AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<Rect32, HostError> {
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let dc = gdi
            .begin_window_paint_dc(
                projection,
                Region32::rectangle(row.whole),
                self.tree_font(window).map_err(HostError::User32)?,
                gdi.system_color(5),
            )
            .map_err(HostError::Gdi)?;
        let result = (|| {
            let item = self
                .tree(window)
                .map_err(HostError::User32)?
                .item(row.id)
                .ok_or(HostError::InvalidRequest)?;
            let canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
            let width = canvas
                .measure_text(gdi, &item.text)
                .map_err(HostError::Gdi)?
                .advance
                .ceil() as i32;
            Ok(Rect32 {
                right: row.label.left.saturating_add(width + 4),
                ..row.label
            })
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, projection.thread)
                .expect("tree owns its measurement dc")
        );
        result
    }
    pub(super) fn draw_tree_background(
        &self,
        window: WindowHandle32,
        dc: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), HostError> {
        let tree = self.tree(window).map_err(HostError::User32)?;
        let extent = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
        let rect = canvas
            .device_rect(Rect32 {
                left: 0,
                top: 0,
                right: extent.width as i32,
                bottom: extent.height as i32,
            })
            .map_err(HostError::Gdi)?;
        canvas.fill(
            gdi,
            rect,
            if tree.background == 0xff000000 {
                gdi.system_color(5)
            } else {
                tree.background
            },
        );
        gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
    }
    pub(super) fn paint_tree(
        &mut self,
        window: WindowHandle32,
        external: u32,
        print: bool,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        if (print || external != 0) && external == 0 {
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
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let tree = self.tree(window).map_err(HostError::User32)?;
        let font = self.tree_font(window).map_err(HostError::User32)?;
        let background = if tree.background == 0xff000000 {
            gdi.system_color(5)
        } else {
            tree.background
        };
        let dc = if external == 0 {
            gdi.begin_window_paint_dc(projection, region, font, background)
                .map_err(HostError::Gdi)?
        } else {
            external
        };
        let result = self.render_tree(window, dc, ram, gdi);
        if external == 0 {
            assert!(
                gdi.release_window_dc(window.get(), dc, thread)
                    .expect("tree owns its paint dc")
            );
        }
        result?;
        if external == 0 {
            self.state
                .set_update_region(window, Region32::default(), false)
                .expect("tree paint was preflighted");
        }
        Ok(User32Outcome32::Return(0))
    }
    fn render_tree(
        &self,
        window: WindowHandle32,
        dc: u32,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), HostError> {
        let rows = self.tree_rows(window, gdi)?;
        let tree = self.tree(window).map_err(HostError::User32)?;
        let font = self.tree_font(window).map_err(HostError::User32)?;
        let style = self.state.record(window).map_err(HostError::User32)?.style;
        let extent = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        let mut canvas = gdi.begin_canvas(ram, dc).map_err(HostError::Gdi)?;
        canvas
            .with_text_style(gdi, font, style & 0x40 != 0, None, |canvas| {
                let bounds = canvas.device_rect(Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                })?;
                let background = if tree.background == 0xff000000 {
                    gdi.system_color(5)
                } else {
                    tree.background
                };
                canvas.fill(gdi, bounds, background);
                for row in rows {
                    let item = tree.item(row.id).expect("same measured item");
                    let selected = tree.selected == row.id
                        && (self.windows().focus() == Some(window) || style & 0x20 != 0)
                        || tree.drop_target == row.id;
                    let text_bounds = row.label;
                    let label_width =
                        canvas.measure_text(gdi, &item.text)?.advance.ceil() as i32 + 4;
                    let highlight = if style & 0x1000 != 0 {
                        row.whole
                    } else {
                        Rect32 {
                            right: row.label.left.saturating_add(label_width),
                            ..row.label
                        }
                    };
                    if selected {
                        canvas.fill(
                            gdi,
                            canvas.device_rect(highlight)?,
                            gdi.system_color(if self.windows().focus() == Some(window) {
                                13
                            } else {
                                15
                            }),
                        );
                    }
                    if style & 2 != 0 && tree.depth(row.id) > 0 {
                        let x = row.button.left + 4;
                        let y = (row.button.top + row.button.bottom) / 2;
                        canvas.fill(
                            gdi,
                            canvas.device_rect(Rect32 {
                                left: x,
                                top: row.whole.top,
                                right: x + 1,
                                bottom: y + 1,
                            })?,
                            gdi.system_color(16),
                        );
                        canvas.fill(
                            gdi,
                            canvas.device_rect(Rect32 {
                                left: x,
                                top: y,
                                right: row.label.left,
                                bottom: y + 1,
                            })?,
                            gdi.system_color(16),
                        );
                    }
                    if style & 1 != 0 && tree.has_children(row.id) {
                        let button = canvas.device_rect(row.button)?;
                        canvas.fill(gdi, button, background);
                        canvas.frame(gdi, button, gdi.system_color(16), gdi.system_color(16));
                        let midx = (row.button.left + row.button.right) / 2;
                        let midy = (row.button.top + row.button.bottom) / 2;
                        canvas.fill(
                            gdi,
                            canvas.device_rect(Rect32 {
                                left: row.button.left + 2,
                                top: midy,
                                right: row.button.right - 2,
                                bottom: midy + 1,
                            })?,
                            gdi.system_color(8),
                        );
                        if item.state & 0x20 == 0 {
                            canvas.fill(
                                gdi,
                                canvas.device_rect(Rect32 {
                                    left: midx,
                                    top: row.button.top + 2,
                                    right: midx + 1,
                                    bottom: row.button.bottom - 2,
                                })?,
                                gdi.system_color(8),
                            );
                        }
                    }
                    canvas.text(
                        gdi,
                        ram,
                        CanvasText32 {
                            units: &item.text,
                            bounds: text_bounds,
                            color: if selected {
                                gdi.system_color(14)
                            } else if item.state & 4 != 0 {
                                gdi.system_color(17)
                            } else if tree.text_color == 0xff000000 {
                                gdi.system_color(8)
                            } else {
                                tree.text_color
                            },
                            background: None,
                            horizontal: 0,
                            middle: true,
                            mnemonic: false,
                            multiline: false,
                        },
                    )?;
                    if selected && self.windows().focus() == Some(window) {
                        canvas.focus(gdi, canvas.device_rect(highlight)?, gdi.system_color(8));
                    }
                }
                Ok(())
            })
            .map_err(HostError::Gdi)?;
        gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
    }
    pub(super) fn tree_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        if !self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .enabled
        {
            return Ok(User32Outcome32::Return(0));
        }
        let tree = self.tree(window).map_err(HostError::User32)?;
        let ids = tree.visible_ids().map_err(HostError::User32)?;
        let current = ids.iter().position(|&id| id == tree.selected);
        let action = match args[1] {
            0x100 => match args[2] {
                0x26 => current
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| ids.get(index))
                    .copied()
                    .or_else(|| ids.first().copied())
                    .map(changes::Change32::Select),
                0x28 => if let Some(index) = current {
                    ids.get((index + 1).min(ids.len() - 1)).copied()
                } else {
                    ids.first().copied()
                }
                .map(changes::Change32::Select),
                0x24 => ids.first().copied().map(changes::Change32::Select),
                0x23 => ids.last().copied().map(changes::Change32::Select),
                0x25 => tree.item(tree.selected).and_then(|item| {
                    if item.state & 0x20 != 0 {
                        Some(changes::Change32::Expand(item.id, false))
                    } else {
                        (item.parent != 0).then_some(changes::Change32::Select(item.parent))
                    }
                }),
                0x27 => tree.item(tree.selected).and_then(|item| {
                    if tree.has_children(item.id) && item.state & 0x20 == 0 {
                        Some(changes::Change32::Expand(item.id, true))
                    } else {
                        let child = tree.first_child(item.id);
                        (child != 0).then_some(changes::Change32::Select(child))
                    }
                }),
                _ => None,
            },
            0x201 | 0x203 => {
                let point = Point32 {
                    x: args[3] as u16 as i16 as i32,
                    y: (args[3] >> 16) as u16 as i16 as i32,
                };
                self.tree_rows(window, gdi)?
                    .iter()
                    .find(|row| inside(row.whole, point))
                    .map(|row| {
                        if self.state.record(window).expect("same tree").style & 1 != 0
                            && inside(row.button, point)
                            || args[1] == 0x203
                        {
                            changes::Change32::Expand(
                                row.id,
                                tree.item(row.id).is_some_and(|item| item.state & 0x20 == 0),
                            )
                        } else {
                            changes::Change32::Select(row.id)
                        }
                    })
            }
            0x115 | 0x20a => {
                if ids.is_empty() {
                    return Ok(User32Outcome32::Return(0));
                }
                let start = ids.iter().position(|&id| id == tree.first).unwrap_or(0);
                let rows = self.tree_rows(window, gdi)?.len().max(1);
                let next = if args[1] == 0x20a {
                    (start as i32 - (args[2] >> 16) as u16 as i16 as i32 / 120 * 3)
                        .clamp(0, ids.len() as i32 - 1) as usize
                } else {
                    match args[2] & 0xffff {
                        0 => start.saturating_sub(1),
                        1 => (start + 1).min(ids.len() - 1),
                        2 => start.saturating_sub(rows),
                        3 => (start + rows).min(ids.len() - 1),
                        4 | 5 => ((args[2] >> 16) as usize).min(ids.len() - 1),
                        6 => 0,
                        7 => ids.len() - 1,
                        _ => start,
                    }
                };
                let first = ids[next];
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tree_mut(window).map_err(HostError::User32)?.first = first;
                self.tree_changed(window);
                return Ok(User32Outcome32::Return(0));
            }
            _ => None,
        };
        if let Some(action) = action {
            return self
                .begin_tree_change(
                    window,
                    action,
                    if args[1] == 0x100 { 2 } else { 1 },
                    args[1] == 0x201,
                    ram,
                    call,
                )
                .map_err(HostError::User32);
        }
        Ok(User32Outcome32::Return(0))
    }
}
fn inside(rect: Rect32, point: Point32) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}
