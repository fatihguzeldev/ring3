use super::*;
use crate::{
    process::windows::gdi::CanvasText32,
    windows::{
        gdi::{GdiContext32, Region32},
        user32::{Point32, Rect32},
    },
};
pub(super) struct Layout32 {
    pub(super) items: Vec<Rect32>,
    rows: u32,
    height: i32,
}
impl User32Context32 {
    pub(in crate::process::windows::user32::common::tab) fn tab_layout(
        &self,
        window: WindowHandle32,
        _ram: &AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<Layout32, HostError> {
        let tab = self.tab(window).map_err(HostError::User32)?;
        let style = self.state.record(window).map_err(HostError::User32)?.style;
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let font = if tab.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            tab.font
        };
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: projection.width as i32,
            bottom: projection.height as i32,
        };
        let dc = gdi
            .begin_window_paint_dc(
                projection,
                Region32::rectangle(bounds),
                font,
                gdi.system_color(15),
            )
            .map_err(HostError::Gdi)?;
        let result = (|| {
            let metrics = gdi.font_metrics(font).map_err(HostError::Gdi)?;
            let height = tab
                .item_size
                .map_or(metrics.height + tab.padding.1.saturating_mul(2), |size| {
                    size.1
                })
                .max(1);
            let mut widths = Vec::new();
            widths
                .try_reserve_exact(tab.items.len())
                .map_err(|_| HostError::User32(E::Allocation))?;
            for item in &tab.items {
                let measured = gdi
                    .measure_text(dc, &item.text)
                    .map_err(HostError::Gdi)?
                    .advance
                    .ceil() as i32;
                widths.push(
                    tab.item_size.map_or(
                        measured
                            .saturating_add(tab.padding.0.saturating_mul(2))
                            .max(tab.min_width)
                            .max(1),
                        |size| size.0,
                    ),
                );
            }
            if style & 0x400 != 0
                && let Some(maximum) = widths.iter().copied().max()
            {
                widths.fill(maximum);
            }
            let mut items = Vec::new();
            items
                .try_reserve_exact(widths.len())
                .map_err(|_| HostError::User32(E::Allocation))?;
            let mut x = 2i32;
            let mut y = 0i32;
            let mut rows = u32::from(!widths.is_empty());
            for width in widths {
                if style & 0x200 != 0 && x > 2 && x.saturating_add(width) > bounds.right {
                    rows += 1;
                    x = 2;
                    y = y.saturating_add(height);
                }
                let top = if style & 2 != 0 {
                    bounds.bottom.saturating_sub(y).saturating_sub(height)
                } else {
                    y
                };
                items.push(Rect32 {
                    left: x,
                    top,
                    right: x.saturating_add(width),
                    bottom: top.saturating_add(height),
                });
                x = x.saturating_add(width);
            }
            Ok(Layout32 {
                items,
                rows,
                height: height.saturating_mul(rows as i32),
            })
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("tab layout owns its acquired dc")
        );
        result
    }
    pub(in crate::process::windows::user32::common::tab) fn tab_geometry_message(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<u32, HostError> {
        let layout = self.tab_layout(window, ram, gdi, thread)?;
        match args[1] {
            0x130a => {
                let Some(rect) = layout.items.get(args[2] as usize) else {
                    return Ok(0);
                };
                memory::write_rect(ram, args[3], *rect).map_err(HostError::User32)?;
                Ok(1)
            }
            0x132c => Ok(layout.rows),
            0x130d => {
                let values = memory::words::<3>(ram, args[3]).map_err(HostError::User32)?;
                let point = Point32 {
                    x: values[0] as i32,
                    y: values[1] as i32,
                };
                let found = layout.items.iter().position(|rect| {
                    point.x >= rect.left
                        && point.x < rect.right
                        && point.y >= rect.top
                        && point.y < rect.bottom
                });
                let flag = if found.is_some() { 2 } else { 1 };
                ram.write(
                    GuestAddress(
                        args[3]
                            .checked_add(8)
                            .ok_or(HostError::User32(E::Win32(87)))?,
                    ),
                    &u32::to_le_bytes(flag),
                )
                .map_err(HostError::Memory)?;
                Ok(found.map_or(u32::MAX, |index| index as u32))
            }
            0x1328 => {
                let mut rect = memory::rect(ram, args[3]).map_err(HostError::User32)?;
                let direction = if args[2] != 0 { 1i32 } else { -1 };
                let top = if self.state.record(window).map_err(HostError::User32)?.style & 2 == 0 {
                    layout.height
                } else {
                    0
                };
                let bottom = if top == 0 { layout.height } else { 0 };
                rect.left = rect.left.saturating_sub(2 * direction);
                rect.right = rect.right.saturating_add(2 * direction);
                rect.top = rect.top.saturating_sub((2 + top) * direction);
                rect.bottom = rect.bottom.saturating_add((2 + bottom) * direction);
                memory::write_rect(ram, args[3], rect).map_err(HostError::User32)?;
                Ok(0)
            }
            _ => Err(HostError::InvalidRequest),
        }
    }
    pub(in crate::process::windows::user32::common::tab) fn paint_tab(
        &mut self,
        window: WindowHandle32,
        external: u32,
        print: bool,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<User32Outcome32, HostError> {
        if print || external != 0 {
            return Err(HostError::User32(E::Win32(50)));
        }
        let (region, _) = self
            .state
            .update_region(window)
            .map_err(HostError::User32)?;
        if region.complexity() == 1 {
            return Ok(User32Outcome32::Return(0));
        }
        self.state.check_paint_change().map_err(HostError::User32)?;
        let layout = self.tab_layout(window, ram, gdi, thread)?;
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let tab = self.tab(window).map_err(HostError::User32)?;
        let font = if tab.font == 0 {
            GdiContext32::stock_object(17).map_err(HostError::Gdi)?
        } else {
            tab.font
        };
        let background = gdi.system_color(15);
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
            let style = self.state.record(window).map_err(HostError::User32)?.style;
            if style & 0x100 == 0 {
                let mut page = bounds;
                if style & 2 == 0 {
                    page.top = layout.height.saturating_sub(1);
                } else {
                    page.bottom = page.bottom.saturating_sub(layout.height);
                }
                canvas.frame(gdi, page, gdi.system_color(20), gdi.system_color(16));
            }
            for (index, rect) in layout.items.iter().copied().enumerate() {
                let selected = index as i32 == tab.selected;
                canvas.frame(
                    gdi,
                    rect,
                    if selected {
                        gdi.system_color(20)
                    } else {
                        gdi.system_color(16)
                    },
                    if selected {
                        gdi.system_color(16)
                    } else {
                        gdi.system_color(20)
                    },
                );
                canvas
                    .text(
                        gdi,
                        ram,
                        CanvasText32 {
                            units: &tab.items[index].text,
                            bounds: Rect32 {
                                left: rect.left + 2,
                                top: rect.top + 1,
                                right: rect.right.saturating_sub(2),
                                bottom: rect.bottom.saturating_sub(1),
                            },
                            color: gdi.system_color(if tab.items[index].state & 2 != 0 {
                                13
                            } else {
                                18
                            }),
                            background: None,
                            horizontal: 1,
                            middle: true,
                            mnemonic: true,
                            multiline: false,
                        },
                    )
                    .map_err(HostError::Gdi)?;
                if index as i32 == tab.focused && self.windows().focus() == Some(window) {
                    canvas.focus(
                        gdi,
                        Rect32 {
                            left: rect.left + 3,
                            top: rect.top + 3,
                            right: rect.right - 3,
                            bottom: rect.bottom - 3,
                        },
                        gdi.system_color(18),
                    );
                }
            }
            gdi.publish_canvas(ram, canvas).map_err(HostError::Gdi)
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("tab paint owns its acquired dc")
        );
        result?;
        self.state
            .set_update_region(window, Region32::default(), false)
            .expect("tab paint was preflighted");
        Ok(User32Outcome32::Return(0))
    }
}

impl User32Context32 {
    pub(in crate::process::windows::user32::common::tab) fn tab_input_index(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &AddressSpace,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<Option<usize>, HostError> {
        let tab = self.tab(window).map_err(HostError::User32)?;
        if tab.items.is_empty() {
            return Ok(None);
        }
        let layout = self.tab_layout(window, ram, gdi, thread)?;
        if args[1] == 0x201 {
            let x = i32::from(args[3] as i16);
            let y = i32::from((args[3] >> 16) as i16);
            return Ok(layout.items.iter().position(|rect| {
                x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
            }));
        }
        let current = tab.selected.max(tab.focused).max(0) as usize;
        let current = current.min(tab.items.len() - 1);
        Ok(match args[2] {
            37 => Some(current.saturating_sub(1)),
            39 => Some((current + 1).min(tab.items.len() - 1)),
            36 => Some(0),
            35 => Some(tab.items.len() - 1),
            38 | 40 => {
                let old = layout.items[current];
                let x = (i64::from(old.left) + i64::from(old.right)) / 2;
                layout
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, rect)| {
                        if args[2] == 38 {
                            rect.top < old.top
                        } else {
                            rect.top > old.top
                        }
                    })
                    .min_by_key(|(_, rect)| {
                        (i64::from(rect.top) - i64::from(old.top)).abs() * 1_000_000
                            + ((i64::from(rect.left) + i64::from(rect.right)) / 2 - x).abs()
                    })
                    .map(|(index, _)| index)
            }
            _ => None,
        })
    }
}
