use super::super::{
    context::GdiRuntime32,
    fonts::rendering::{TextPixels32, TextRequest32},
};
use crate::{
    memory::AddressSpace,
    windows::{
        gdi::GdiError32,
        user32::{Point32, Rect32},
    },
};

pub(super) struct Canvas32 {
    prepared: Option<TextPixels32>,
    dc: u32,
}
impl Canvas32 {
    pub(super) fn new(
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        dc: u32,
    ) -> Result<Self, GdiError32> {
        let index = gdi.dc_surface(dc)?;
        let surface = &gdi.surfaces[index];
        Ok(Self {
            dc,
            prepared: Some(TextPixels32 {
                index,
                pixels: surface.storage.read(memory, surface.layout.byte_len()?)?,
                damage: Rect32::default(),
                state: gdi.model.state(dc)?.clone(),
            }),
        })
    }
    pub(super) fn dc(&self) -> u32 {
        self.dc
    }
    pub(super) fn set_background_color(&mut self, color: u32) {
        self.prepared.as_mut().unwrap().state.background_color = color;
    }
    pub(super) fn fill(&mut self, gdi: &GdiRuntime32, rect: Rect32, color: u32) {
        let prepared = self.prepared.as_mut().unwrap();
        let surface = &gdi.surfaces[prepared.index];
        let rect = intersect(rect, super::super::bitmaps::bounds(surface.layout));
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                if prepared.state.accepts(
                    Point32 { x, y },
                    super::super::bitmaps::bounds(surface.layout),
                ) {
                    surface.layout.write_pixel(
                        &mut prepared.pixels,
                        &surface.palette,
                        x as u32,
                        y as u32,
                        color,
                    );
                    prepared.damage = union(
                        prepared.damage,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 1,
                        },
                    );
                }
            }
        }
    }
    pub(super) fn frame(&mut self, gdi: &GdiRuntime32, rect: Rect32, top: u32, bottom: u32) {
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return;
        }
        self.fill(
            gdi,
            Rect32 {
                bottom: rect.top + 1,
                ..rect
            },
            top,
        );
        self.fill(
            gdi,
            Rect32 {
                right: rect.left + 1,
                ..rect
            },
            top,
        );
        self.fill(
            gdi,
            Rect32 {
                top: rect.bottom - 1,
                ..rect
            },
            bottom,
        );
        self.fill(
            gdi,
            Rect32 {
                left: rect.right - 1,
                ..rect
            },
            bottom,
        );
    }
    pub(super) fn focus(&mut self, gdi: &GdiRuntime32, rect: Rect32, color: u32) {
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return;
        }
        for x in (rect.left..rect.right).step_by(2) {
            self.fill(
                gdi,
                Rect32 {
                    left: x,
                    top: rect.top,
                    right: x + 1,
                    bottom: rect.top + 1,
                },
                color,
            );
            self.fill(
                gdi,
                Rect32 {
                    left: x,
                    top: rect.bottom - 1,
                    right: x + 1,
                    bottom: rect.bottom,
                },
                color,
            );
        }
        for y in (rect.top..rect.bottom).step_by(2) {
            self.fill(
                gdi,
                Rect32 {
                    left: rect.left,
                    top: y,
                    right: rect.left + 1,
                    bottom: y + 1,
                },
                color,
            );
            self.fill(
                gdi,
                Rect32 {
                    left: rect.right - 1,
                    top: y,
                    right: rect.right,
                    bottom: y + 1,
                },
                color,
            );
        }
    }
    pub(super) fn background(
        &mut self,
        gdi: &GdiRuntime32,
        rect: Rect32,
        brush: u32,
    ) -> Result<(), GdiError32> {
        let prepared = self.prepared.as_mut().unwrap();
        let surface = &gdi.surfaces[prepared.index];
        let rect = intersect(rect, super::super::bitmaps::bounds(surface.layout));
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                let point = Point32 { x, y };
                if !prepared
                    .state
                    .accepts(point, super::super::bitmaps::bounds(surface.layout))
                {
                    continue;
                }
                if let Some(color) = gdi.brush(&prepared.state, brush, point)? {
                    surface.layout.write_pixel(
                        &mut prepared.pixels,
                        &surface.palette,
                        x as u32,
                        y as u32,
                        color,
                    );
                    prepared.damage = union(
                        prepared.damage,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 1,
                        },
                    );
                }
            }
        }
        Ok(())
    }
    pub(super) fn text(
        &mut self,
        gdi: &GdiRuntime32,
        memory: &AddressSpace,
        text: ControlText32<'_>,
    ) -> Result<(), GdiError32> {
        if text.units.is_empty() {
            return Ok(());
        }
        let flags = text.horizontal
            | if text.multiline { 16 } else { 32 }
            | if text.mnemonic { 0 } else { 0x800 };
        let paragraph = gdi.prepare_paragraph(self.dc, text.units, text.bounds, flags, 8)?;
        let mut y = if text.middle {
            text.bounds.top + (text.bounds.bottom - text.bounds.top - paragraph.height) / 2
        } else {
            text.bounds.top
        };
        for line in &paragraph.lines {
            let x = match text.horizontal {
                1 => text.bounds.left + (text.bounds.right - text.bounds.left - line.width) / 2,
                2 => text.bounds.right - line.width,
                _ => text.bounds.left,
            };
            let seed = self.prepared.take().unwrap();
            let mut state = seed.state.clone();
            state.text_color = text.color;
            if let Some(color) = text.background {
                state.background_color = color;
            }
            state.text_align = 0;
            self.prepared = Some(gdi.prepare_text_from(
                memory,
                TextRequest32 {
                    dc: self.dc,
                    point: Point32 { x, y },
                    units: &line.units,
                    options: 4,
                    rectangle: Some(text.bounds),
                    underlines: &line.underlines,
                    prefix_only: false,
                    advances: line.advances.as_deref(),
                },
                state,
                Some(seed),
            )?);
            y = y
                .checked_add(paragraph.line_height)
                .ok_or(GdiError32::InvalidRequest)?;
            if y >= text.bounds.bottom {
                break;
            }
        }
        Ok(())
    }
    pub(super) fn finish(self) -> TextPixels32 {
        self.prepared.unwrap()
    }
}
pub(super) fn inset(rect: Rect32, amount: i32) -> Rect32 {
    Rect32 {
        left: (rect.left + amount).min(rect.right),
        top: (rect.top + amount).min(rect.bottom),
        right: (rect.right - amount).max(rect.left),
        bottom: (rect.bottom - amount).max(rect.top),
    }
}
fn intersect(a: Rect32, b: Rect32) -> Rect32 {
    Rect32 {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    }
}
fn union(a: Rect32, b: Rect32) -> Rect32 {
    if a.left >= a.right || a.top >= a.bottom {
        b
    } else {
        Rect32 {
            left: a.left.min(b.left),
            top: a.top.min(b.top),
            right: a.right.max(b.right),
            bottom: a.bottom.max(b.bottom),
        }
    }
}

pub(super) struct ControlText32<'a> {
    pub units: &'a [u16],
    pub bounds: Rect32,
    pub color: u32,
    pub background: Option<u32>,
    pub horizontal: u32,
    pub middle: bool,
    pub mnemonic: bool,
    pub multiline: bool,
}
