use super::layout::{ItemSize32, MenuLayout32};
use crate::{
    memory::AddressSpace,
    process::windows::gdi::{CanvasText32, GdiCanvas32, GdiRuntime32},
    windows::user32::{
        Point32, Rect32, User32Error32 as E,
        menus::{MenuHandle32, MenuStore32},
    },
};

#[derive(Clone, Copy)]
pub(super) struct OwnerSize32 {
    pub revision: u64,
    pub width: i32,
    pub height: i32,
}

pub(super) struct MenuVisual32 {
    pub layout: MenuLayout32,
    pub revision: u64,
    pub popup: bool,
    pub gutter: i32,
    pub padding: i32,
}

impl MenuVisual32 {
    pub(super) fn measure(
        gdi: &GdiRuntime32,
        dc: u32,
        menus: &MenuStore32,
        handle: MenuHandle32,
        popup: bool,
        width: i32,
        owned: &[Option<OwnerSize32>],
    ) -> Result<Self, E> {
        let metrics = gdi.measure_text(dc, &[]).map_err(gdi_error)?.metrics;
        let padding = (metrics.average_width / 2).max(2);
        let gutter = metrics.height.max(1);
        let row_height = metrics.height.checked_add(4).ok_or(E::Capacity)?;
        let count = menus.count(handle)?;
        let mut sizes = Vec::new();
        sizes.try_reserve_exact(count).map_err(|_| E::Allocation)?;
        let mut text = Vec::new();
        for index in 0..count {
            let item = menus.item(handle, index as u32, true)?;
            if item.kind & (4 | 0x2000) != 0 || item.bitmap != 0 {
                return Err(E::Win32(50));
            }
            if item.kind & 0x100 != 0 {
                let size = owned
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|size| size.revision == menus.revision())
                    .ok_or(E::InvalidRequest)?;
                sizes.push(ItemSize32 {
                    width: size.width,
                    height: size.height,
                    kind: item.kind,
                });
                continue;
            }
            let (label, shortcut) = parts(&item.text);
            visible_label(label, &mut text)?;
            let label_width = gdi
                .measure_text(dc, &text)
                .map_err(gdi_error)?
                .advance
                .ceil() as i32;
            let shortcut_width = gdi
                .measure_text(dc, shortcut)
                .map_err(gdi_error)?
                .advance
                .ceil() as i32;
            let width = if popup {
                label_width
                    .checked_add(shortcut_width)
                    .and_then(|n| n.checked_add(gutter * 2 + padding * 3))
            } else {
                label_width.checked_add(padding * 2)
            }
            .ok_or(E::Capacity)?;
            sizes.push(ItemSize32 {
                width: width.max(1),
                height: if item.kind & 0x800 != 0 {
                    (row_height / 2).max(4)
                } else {
                    row_height
                },
                kind: item.kind,
            });
        }
        let layout = if popup {
            MenuLayout32::popup(&sizes, 1)?
        } else {
            MenuLayout32::bar(&sizes, width)?
        };
        Ok(Self {
            layout,
            revision: menus.revision(),
            popup,
            gutter,
            padding,
        })
    }

    pub(super) fn draw(
        &self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        canvas: &mut GdiCanvas32,
        menus: &MenuStore32,
        paint: MenuPaint32,
    ) -> Result<(), E> {
        let MenuPaint32 {
            handle,
            origin,
            selected,
            dirty,
        } = paint;
        let color = |index| gdi.system_color(index);
        let bounds = Rect32 {
            left: origin.x,
            top: origin.y,
            right: origin
                .x
                .checked_add(self.layout.extent.x)
                .ok_or(E::Capacity)?,
            bottom: origin
                .y
                .checked_add(self.layout.extent.y)
                .ok_or(E::Capacity)?,
        };
        if dirty.is_none() {
            canvas.fill(gdi, bounds, color(4));
        }
        if self.popup && dirty.is_none() {
            canvas.frame(gdi, bounds, color(20), color(16));
        }
        for (index, _) in self.layout.items.iter().enumerate() {
            if dirty.is_some_and(|positions| !positions.contains(&Some(index))) {
                continue;
            }
            let item = menus.item(handle, index as u32, true)?;
            if item.kind & (4 | 0x2000) != 0 || item.bitmap != 0 {
                return Err(E::Win32(50));
            }
            if item.kind & 0x100 != 0 {
                continue;
            }
            let rect = self.layout.screen_item(index, origin)?;
            if item.kind & 0x800 != 0 {
                let y = rect.top + (rect.bottom - rect.top) / 2;
                let separator = Rect32 {
                    left: rect.left + 2,
                    top: y,
                    right: (rect.right - 2).max(rect.left + 2),
                    bottom: y + 1,
                };
                canvas.fill(gdi, separator, color(16));
                canvas.fill(
                    gdi,
                    Rect32 {
                        top: y + 1,
                        bottom: y + 2,
                        ..separator
                    },
                    color(20),
                );
                continue;
            }
            let selected = selected == Some(index) || item.state & 0x80 != 0;
            let background = color(if selected { 13 } else { 4 });
            let foreground = color(if item.state & 3 != 0 {
                17
            } else if selected {
                14
            } else {
                7
            });
            canvas.fill(gdi, rect, background);
            let (label, shortcut) = parts(&item.text);
            let text_bounds = Rect32 {
                left: rect.left + self.padding + if self.popup { self.gutter } else { 0 },
                top: rect.top,
                right: rect.right - self.padding - if self.popup { self.gutter } else { 0 },
                bottom: rect.bottom,
            };
            canvas
                .text(
                    gdi,
                    memory,
                    CanvasText32 {
                        units: label,
                        bounds: text_bounds,
                        color: foreground,
                        background: Some(background),
                        horizontal: 0,
                        middle: true,
                        mnemonic: true,
                        multiline: false,
                    },
                )
                .map_err(gdi_error)?;
            if self.popup && !shortcut.is_empty() {
                canvas
                    .text(
                        gdi,
                        memory,
                        CanvasText32 {
                            units: shortcut,
                            bounds: text_bounds,
                            color: foreground,
                            background: Some(background),
                            horizontal: 2,
                            middle: true,
                            mnemonic: false,
                            multiline: false,
                        },
                    )
                    .map_err(gdi_error)?;
            }
            if self.popup && item.state & 8 != 0 {
                let x = rect.left + self.padding + self.gutter / 2 - 3;
                let y = rect.top + (rect.bottom - rect.top) / 2;
                for (offset, dy) in [(0, 0), (1, 1), (2, 2), (3, 1), (4, 0), (5, -1), (6, -2)] {
                    canvas.fill(
                        gdi,
                        Rect32 {
                            left: x + offset,
                            top: y + dy,
                            right: x + offset + 1,
                            bottom: y + dy + 2,
                        },
                        foreground,
                    );
                }
            }
            if self.popup && item.submenu.is_some() {
                let x = rect.right - self.padding - self.gutter / 2 - 2;
                let y = rect.top + (rect.bottom - rect.top) / 2;
                for offset in 0..4 {
                    canvas.fill(
                        gdi,
                        Rect32 {
                            left: x + offset,
                            top: y - 3 + offset,
                            right: x + offset + 1,
                            bottom: y + 4 - offset,
                        },
                        foreground,
                    );
                }
            }
        }
        Ok(())
    }
}

fn parts(text: &[u16]) -> (&[u16], &[u16]) {
    text.iter()
        .position(|&unit| unit == 9 || unit == 8)
        .map_or((text, &[]), |index| (&text[..index], &text[index + 1..]))
}

fn visible_label(input: &[u16], output: &mut Vec<u16>) -> Result<(), E> {
    output.clear();
    output.try_reserve(input.len()).map_err(|_| E::Allocation)?;
    let mut index = 0;
    while index < input.len() {
        if input[index] == u16::from(b'&') {
            index += 1;
            if index == input.len() {
                break;
            }
        }
        output.push(input[index]);
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_text_measurement_keeps_escaped_ampersands_and_separates_shortcut_text() {
        let units: Vec<u16> = "&file && more\tCtrl+F".encode_utf16().collect();
        let (label, shortcut) = parts(&units);
        let mut visible = Vec::new();
        visible_label(label, &mut visible).unwrap();
        assert_eq!(String::from_utf16(&visible).unwrap(), "file & more");
        assert_eq!(String::from_utf16(shortcut).unwrap(), "Ctrl+F");
    }
}

pub(super) fn gdi_error(error: crate::windows::gdi::GdiError32) -> E {
    match error {
        crate::windows::gdi::GdiError32::Memory(error) => E::Memory(error),
        crate::windows::gdi::GdiError32::Allocation => E::Allocation,
        error => E::Win32(error.win32()),
    }
}

pub(super) struct MenuPaint32 {
    pub handle: MenuHandle32,
    pub origin: Point32,
    pub selected: Option<usize>,
    pub dirty: Option<[Option<usize>; 2]>,
}
