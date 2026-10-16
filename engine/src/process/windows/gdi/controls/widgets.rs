use super::super::context::GdiRuntime32;
use super::super::{CanvasText32, GdiCanvas32, raster::canvas::inset};
use super::ControlVisual32;
use crate::{
    memory::AddressSpace,
    windows::{controls::ControlKind32, gdi::GdiError32, user32::Rect32},
};

pub(super) fn paint(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    dc: u32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
) -> Result<(), GdiError32> {
    let push_button = visual.kind == ControlKind32::Button
        && (matches!(visual.style & 15, 0 | 1) || visual.style & 0x1000 != 0);
    let color = if visual.enabled {
        if push_button {
            gdi.system_color(18)
        } else {
            gdi.model.state(dc)?.text_color
        }
    } else {
        gdi.system_color(17)
    };
    match visual.kind {
        ControlKind32::Button => button(gdi, memory, canvas, visual, bounds, color),
        ControlKind32::Edit => edit(gdi, memory, canvas, dc, visual, bounds, color),
        ControlKind32::Static => static_control(gdi, memory, canvas, visual, bounds, color),
        ControlKind32::ListBox => list(gdi, memory, canvas, dc, visual, bounds, color),
        ControlKind32::ComboBox => combo(gdi, memory, canvas, dc, visual, bounds, color),
        ControlKind32::ScrollBar => {
            scrollbar(gdi, canvas, visual, bounds);
            Ok(())
        }
    }
}
fn text<'a>(units: &'a [u16], bounds: Rect32, color: u32) -> CanvasText32<'a> {
    CanvasText32 {
        units,
        bounds,
        color,
        background: None,
        horizontal: 0,
        middle: false,
        mnemonic: false,
        multiline: false,
    }
}
fn button(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
    color: u32,
) -> Result<(), GdiError32> {
    let kind = visual.style & 15;
    let push = matches!(kind, 0 | 1) || visual.style & 0x1000 != 0;
    let mut label = inset(bounds, 3);
    if push {
        let pressed =
            visual.control.pressed || visual.style & 0x1000 != 0 && visual.control.check != 0;
        let (top, bottom) = if pressed {
            (gdi.system_color(21), gdi.system_color(20))
        } else {
            (gdi.system_color(20), gdi.system_color(21))
        };
        canvas.frame(gdi, bounds, top, bottom);
        canvas.frame(
            gdi,
            inset(bounds, 1),
            if pressed {
                gdi.system_color(16)
            } else {
                gdi.system_color(22)
            },
            if pressed {
                gdi.system_color(22)
            } else {
                gdi.system_color(16)
            },
        );
        if kind == 1 {
            canvas.frame(gdi, inset(bounds, 2), 0, 0);
            label = inset(bounds, 4);
        }
        if pressed {
            label.left += 1;
            label.top += 1;
        }
    } else if kind == 7 {
        let height = gdi.measure_text(canvas.dc(), &[])?.metrics.height;
        let frame = Rect32 {
            top: bounds.top + height / 2,
            ..bounds
        };
        canvas.frame(gdi, frame, gdi.system_color(16), gdi.system_color(20));
        label = Rect32 {
            left: bounds.left + 7,
            bottom: bounds.top + height,
            ..bounds
        };
    } else {
        let size = 13.min(bounds.bottom - bounds.top).max(1);
        let left = if visual.style & 0x20 != 0 {
            bounds.right - size
        } else {
            bounds.left
        };
        let top = bounds.top + (bounds.bottom - bounds.top - size) / 2;
        let box_rect = Rect32 {
            left,
            top,
            right: left + size,
            bottom: top + size,
        };
        canvas.fill(gdi, box_rect, gdi.system_color(5));
        if matches!(kind, 4 | 9) {
            let center = size / 2;
            for y in 0..size {
                for x in 0..size {
                    let distance = (x - center) * (x - center) + (y - center) * (y - center);
                    let radius = center * center;
                    if distance <= radius && distance >= (center - 1).max(0).pow(2)
                        || visual.control.check != 0 && distance <= (center / 2).pow(2)
                    {
                        canvas.fill(
                            gdi,
                            Rect32 {
                                left: left + x,
                                top: top + y,
                                right: left + x + 1,
                                bottom: top + y + 1,
                            },
                            color,
                        );
                    }
                }
            }
        } else {
            canvas.frame(gdi, box_rect, gdi.system_color(21), gdi.system_color(20));
            if visual.control.check == 2 {
                canvas.fill(gdi, inset(box_rect, 3), gdi.system_color(16));
            } else if visual.control.check != 0 {
                for step in 0..7 {
                    let x = left + 3 + step;
                    let y = top + if step < 3 { 5 + step } else { 10 - step };
                    canvas.fill(
                        gdi,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 2,
                        },
                        color,
                    );
                }
            }
        }
        if visual.style & 0x20 != 0 {
            label.right = left - 3;
        } else {
            label.left = box_rect.right + 4;
        }
    }
    let mut request = text(visual.text, label, color);
    request.mnemonic = true;
    request.multiline = visual.style & 0x2000 != 0;
    request.middle = kind != 7;
    request.horizontal = match visual.style & 0x300 {
        0x100 => 0,
        0x200 => 2,
        0x300 => 1,
        _ => u32::from(push),
    };
    canvas.text(gdi, memory, request)?;
    if visual.focused && visual.enabled && kind != 7 {
        canvas.focus(
            gdi,
            label,
            if push {
                gdi.model.state(canvas.dc())?.text_color
            } else {
                0
            },
        );
    }
    Ok(())
}
fn edit(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    dc: u32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
    color: u32,
) -> Result<(), GdiError32> {
    canvas.frame(gdi, bounds, gdi.system_color(16), gdi.system_color(20));
    let area = inset(bounds, 2);
    let password;
    let units = if visual.style & 0x20 != 0 {
        if visual.text.len() > 8192 {
            return Err(GdiError32::Capacity);
        }
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(visual.text.len())
            .map_err(|_| GdiError32::Allocation)?;
        encoded.resize(visual.text.len(), u16::from(b'*'));
        password = encoded;
        password.as_slice()
    } else {
        visual.text
    };
    let selected = visual.control.selection;
    let show = visual.focused || visual.style & 0x100 != 0;
    if show && selected.0 != selected.1 {
        if visual.style & 4 != 0 {
            return Err(GdiError32::Unsupported);
        }
        let start = selected.0.min(selected.1).min(units.len());
        let end = selected.0.max(selected.1).min(units.len());
        let x = area.left + gdi.measure_text(dc, &units[..start])?.advance.round() as i32;
        let right = area.left + gdi.measure_text(dc, &units[..end])?.advance.round() as i32;
        let height = gdi.measure_text(dc, &[])?.metrics.height;
        canvas.fill(
            gdi,
            Rect32 {
                left: x,
                top: area.top,
                right,
                bottom: (area.top + height).min(area.bottom),
            },
            gdi.system_color(13),
        );
        canvas.text(
            gdi,
            memory,
            text(&units[..start], Rect32 { right: x, ..area }, color),
        )?;
        let mut highlighted = text(
            &units[start..end],
            Rect32 {
                left: x,
                right,
                ..area
            },
            gdi.system_color(14),
        );
        highlighted.background = Some(gdi.system_color(13));
        canvas.text(gdi, memory, highlighted)?;
        canvas.text(
            gdi,
            memory,
            text(
                &units[end..],
                Rect32 {
                    left: right,
                    ..area
                },
                color,
            ),
        )?;
    } else {
        let mut request = text(units, area, color);
        request.multiline = visual.style & 4 != 0;
        request.horizontal = match visual.style & 3 {
            1 => 1,
            2 => 2,
            _ => 0,
        };
        canvas.text(gdi, memory, request)?;
        if visual.focused && visual.enabled && visual.style & 0x800 == 0 {
            let at = selected.1.min(units.len());
            let x = area.left + gdi.measure_text(dc, &units[..at])?.advance.round() as i32;
            let height = gdi.measure_text(dc, &[])?.metrics.height;
            canvas.fill(
                gdi,
                Rect32 {
                    left: x,
                    top: area.top,
                    right: x + 1,
                    bottom: (area.top + height).min(area.bottom),
                },
                color,
            );
        }
    }
    Ok(())
}
fn static_control(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
    color: u32,
) -> Result<(), GdiError32> {
    match visual.style & 31 {
        4..=6 => canvas.fill(
            gdi,
            bounds,
            [0, 0x808080, 0xffffff][(visual.style & 31) as usize - 4],
        ),
        7..=9 => {
            let color = [0, 0x808080, 0xffffff][(visual.style & 31) as usize - 7];
            canvas.frame(gdi, bounds, color, color);
        }
        16 => {
            canvas.fill(
                gdi,
                Rect32 {
                    bottom: bounds.top + 1,
                    ..bounds
                },
                gdi.system_color(16),
            );
            canvas.fill(
                gdi,
                Rect32 {
                    top: bounds.top + 1,
                    bottom: bounds.top + 2,
                    ..bounds
                },
                gdi.system_color(20),
            );
        }
        17 => {
            canvas.fill(
                gdi,
                Rect32 {
                    right: bounds.left + 1,
                    ..bounds
                },
                gdi.system_color(16),
            );
            canvas.fill(
                gdi,
                Rect32 {
                    left: bounds.left + 1,
                    right: bounds.left + 2,
                    ..bounds
                },
                gdi.system_color(20),
            );
        }
        18 => canvas.frame(gdi, bounds, gdi.system_color(16), gdi.system_color(20)),
        kind => {
            let mut request = text(visual.text, bounds, color);
            request.horizontal = match kind {
                1 => 1,
                2 => 2,
                _ => 0,
            };
            request.multiline = matches!(kind, 0..=2);
            request.mnemonic = visual.style & 0x80 == 0;
            request.middle = visual.style & 0x200 != 0;
            canvas.text(gdi, memory, request)?;
        }
    }
    Ok(())
}
fn list(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    dc: u32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
    color: u32,
) -> Result<(), GdiError32> {
    canvas.frame(gdi, bounds, gdi.system_color(16), gdi.system_color(20));
    let area = inset(bounds, 1);
    let height = gdi.measure_text(dc, &[])?.metrics.height.max(1);
    let first = visual.control.scroll.2.max(0) as usize;
    for (index, units) in visual.control.items.iter().enumerate().skip(first) {
        let top = area.top
            + ((index - first) as i32)
                .checked_mul(height)
                .ok_or(GdiError32::Capacity)?;
        if top >= area.bottom {
            break;
        }
        let row = Rect32 {
            top,
            bottom: (top + height).min(area.bottom),
            ..area
        };
        let selected = visual.control.selected == Some(index);
        if selected {
            canvas.fill(gdi, row, gdi.system_color(13));
        }
        let mut request = text(
            units,
            Rect32 {
                left: row.left + 2,
                ..row
            },
            if selected {
                gdi.system_color(14)
            } else {
                color
            },
        );
        if selected {
            request.background = Some(gdi.system_color(13));
        }
        canvas.text(gdi, memory, request)?;
    }
    Ok(())
}
fn combo(
    gdi: &GdiRuntime32,
    memory: &AddressSpace,
    canvas: &mut GdiCanvas32,
    dc: u32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
    color: u32,
) -> Result<(), GdiError32> {
    canvas.frame(gdi, bounds, gdi.system_color(16), gdi.system_color(20));
    let height = gdi.measure_text(dc, &[])?.metrics.height.max(1) + 4;
    let simple = visual.style & 3 == 1;
    let top = Rect32 {
        bottom: (bounds.top + height).min(bounds.bottom),
        ..bounds
    };
    let units = visual
        .control
        .selected
        .and_then(|index| visual.control.items.get(index))
        .map_or(visual.text, Vec::as_slice);
    let area = Rect32 {
        right: if simple {
            top.right - 2
        } else {
            top.right - 16
        },
        ..inset(top, 2)
    };
    let highlighted = visual.focused && visual.style & 3 == 3;
    if highlighted {
        canvas.fill(gdi, area, gdi.system_color(13));
    }
    let mut request = text(
        units,
        area,
        if highlighted {
            gdi.system_color(14)
        } else {
            color
        },
    );
    if highlighted {
        request.background = Some(gdi.system_color(13));
    }
    request.middle = true;
    canvas.text(gdi, memory, request)?;
    if simple {
        list(
            gdi,
            memory,
            canvas,
            dc,
            visual,
            Rect32 {
                top: top.bottom,
                ..bounds
            },
            color,
        )?;
    } else {
        let button = Rect32 {
            left: (top.right - 16).max(top.left),
            ..top
        };
        canvas.fill(gdi, button, gdi.system_color(15));
        canvas.frame(gdi, button, gdi.system_color(20), gdi.system_color(21));
        let middle = (button.left + button.right) / 2;
        let y = (button.top + button.bottom) / 2 - 1;
        for row in 0..4 {
            canvas.fill(
                gdi,
                Rect32 {
                    left: middle - 3 + row,
                    top: y + row,
                    right: middle + 4 - row,
                    bottom: y + row + 1,
                },
                color,
            );
        }
    }
    Ok(())
}
fn scrollbar(
    gdi: &GdiRuntime32,
    canvas: &mut GdiCanvas32,
    visual: &ControlVisual32<'_>,
    bounds: Rect32,
) {
    let vertical = visual.style & 1 != 0;
    let extent = if vertical {
        bounds.bottom - bounds.top
    } else {
        bounds.right - bounds.left
    };
    let cross = if vertical {
        bounds.right - bounds.left
    } else {
        bounds.bottom - bounds.top
    };
    let arrow = cross.min(extent / 2).max(0);
    let rect = |start, length| {
        if vertical {
            Rect32 {
                top: bounds.top + start,
                bottom: bounds.top + start + length,
                ..bounds
            }
        } else {
            Rect32 {
                left: bounds.left + start,
                right: bounds.left + start + length,
                ..bounds
            }
        }
    };
    for (end, position) in [0, extent - arrow].into_iter().enumerate() {
        let area = rect(position, arrow);
        canvas.fill(gdi, area, gdi.system_color(15));
        canvas.frame(gdi, area, gdi.system_color(20), gdi.system_color(21));
        let center_x = (area.left + area.right) / 2;
        let center_y = (area.top + area.bottom) / 2;
        let size = ((arrow - 4) / 2).clamp(0, 3);
        for row in 0..size {
            let sign = if end == 0 { -1 } else { 1 };
            let start = row * sign;
            let breadth = size - row - 1;
            let glyph = if vertical {
                Rect32 {
                    left: center_x - breadth,
                    top: center_y + start,
                    right: center_x + breadth + 1,
                    bottom: center_y + start + 1,
                }
            } else {
                Rect32 {
                    left: center_x + start,
                    top: center_y - breadth,
                    right: center_x + start + 1,
                    bottom: center_y + breadth + 1,
                }
            };
            canvas.fill(
                gdi,
                glyph,
                if visual.enabled {
                    gdi.system_color(18)
                } else {
                    gdi.system_color(17)
                },
            );
        }
    }
    let (minimum, maximum, position, page) = visual.control.scroll;
    let range = (i64::from(maximum) - i64::from(minimum) + 1).max(1);
    let track = (extent - arrow * 2).max(0);
    let thumb = if range <= 1 {
        track
    } else {
        ((i64::from(track) * i64::from(page.max(1)) / range) as i32).clamp(8.min(track), track)
    };
    let available = (range - i64::from(page.saturating_sub(1))).max(1);
    let offset = (i64::from(position) - i64::from(minimum)).clamp(0, available)
        * i64::from(track - thumb)
        / available;
    let area = rect(arrow + offset as i32, thumb);
    canvas.fill(gdi, area, gdi.system_color(15));
    canvas.frame(gdi, area, gdi.system_color(20), gdi.system_color(21));
}
