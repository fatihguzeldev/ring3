use super::{
    super::{bitmaps::bounds, context::GdiRuntime32},
    queries::apply_spacing,
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{DcState32, GdiError32, GlyphRaster32, ShapedText32},
        user32::{Point32, Rect32},
    },
};

pub(in crate::process::windows::gdi) struct TextRequest32<'a> {
    pub dc: u32,
    pub point: Point32,
    pub units: &'a [u16],
    pub options: u32,
    pub rectangle: Option<Rect32>,
    pub underlines: &'a [usize],
    pub prefix_only: bool,
    pub advances: Option<&'a [(i32, i32)]>,
}
pub(in crate::process::windows::gdi) struct TextPixels32 {
    pub index: usize,
    pub pixels: Vec<u8>,
    pub damage: Rect32,
    pub state: DcState32,
}
impl GdiRuntime32 {
    pub(super) fn prepare_text(
        &self,
        memory: &AddressSpace,
        request: TextRequest32<'_>,
    ) -> Result<TextPixels32, GdiError32> {
        if request.options & !(2 | 4 | 16 | 128 | 4096 | 8192) != 0 {
            return Err(GdiError32::Unsupported);
        }
        if request.options & (2 | 4) != 0 && request.rectangle.is_none() {
            return Err(GdiError32::InvalidRequest);
        }
        let state = self.model.state(request.dc)?.clone();
        self.prepare_text_from(memory, request, state, None)
    }
    pub(in crate::process::windows::gdi) fn prepare_text_from(
        &self,
        memory: &AddressSpace,
        request: TextRequest32<'_>,
        mut state: DcState32,
        seed: Option<TextPixels32>,
    ) -> Result<TextPixels32, GdiError32> {
        let font = self.logical_font(state.font)?;
        let direct = request.options & (16 | 4096) != 0;
        let converted;
        let units = if request.options & 4096 != 0 && request.options & 16 == 0 {
            converted = char::decode_utf16(request.units.iter().copied())
                .map(|character| {
                    self.fonts
                        .glyph_index(&font, character.unwrap_or('\u{fffd}'))
                })
                .collect::<Vec<_>>();
            converted.as_slice()
        } else {
            request.units
        };
        let mut shaped = self.fonts.shape(
            &font,
            units,
            direct,
            request.options & 128 != 0 || state.text_align & 256 != 0,
        )?;
        if let Some(advances) = request.advances {
            if advances.len() != units.len() {
                return Err(GdiError32::InvalidRequest);
            }
            override_advances(&mut shaped, advances)?;
        } else {
            apply_spacing(&mut shaped, &state, units);
        }
        if !shaped.advance.is_finite() || shaped.advance.abs() > i32::MAX as f32 {
            return Err(GdiError32::InvalidRequest);
        }
        let mut point = if state.text_align & 1 != 0 {
            state.current_position
        } else {
            request.point
        };
        point.x = point
            .x
            .checked_sub(match state.text_align & 6 {
                2 => shaped.advance.round() as i32,
                6 => (shaped.advance * 0.5).round() as i32,
                _ => 0,
            })
            .ok_or(GdiError32::InvalidRequest)?;
        let reference = if state.text_align & 1 != 0 {
            state.current_position
        } else {
            request.point
        };
        let rotation = super::rotation::TextRotation32::new(font.escapement, reference);
        let baseline = point
            .y
            .checked_add(match state.text_align & 24 {
                24 => 0,
                8 => -shaped.metrics.descent,
                _ => shaped.metrics.ascent,
            })
            .ok_or(GdiError32::InvalidRequest)?;
        let index = self.dc_surface(request.dc)?;
        let surface = &self.surfaces[index];
        let layout = surface.layout;
        let (mut pixels, mut damage) = if let Some(seed) = seed {
            if seed.index != index {
                return Err(GdiError32::InvalidRequest);
            }
            (seed.pixels, seed.damage)
        } else {
            (
                surface.storage.read(memory, layout.byte_len()?)?,
                Rect32::default(),
            )
        };
        let clip = request
            .rectangle
            .filter(|_| request.options & 4 != 0)
            .map(|rectangle| device_rect(&state, rectangle))
            .transpose()?;
        let mut paint = |source: Rect32,
                         color: u32,
                         coverage: u8,
                         rotation: super::rotation::TextRotation32|
         -> Result<(), GdiError32> {
            let rectangle = rotation.device_bounds(&state, source)?;
            let rectangle = intersect(rectangle, bounds(layout));
            let rectangle = clip.map_or(rectangle, |clip| intersect(rectangle, clip));
            for y in rectangle.top..rectangle.bottom {
                for x in rectangle.left..rectangle.right {
                    let point = Point32 { x, y };
                    if !state.accepts(point, bounds(layout))
                        || !rotation.accepts(&state, source, point)?
                    {
                        continue;
                    }
                    let old = layout.read_pixel(&pixels, &surface.palette, x as u32, y as u32);
                    let color = blend(old, color, coverage);
                    layout.write_pixel(&mut pixels, &surface.palette, x as u32, y as u32, color);
                    damage = union(
                        damage,
                        Rect32 {
                            left: x,
                            top: y,
                            right: x + 1,
                            bottom: y + 1,
                        },
                    );
                }
            }
            Ok(())
        };
        if request.options & 2 != 0 && !request.prefix_only {
            paint(
                request.rectangle.unwrap(),
                state.background_color,
                255,
                super::rotation::TextRotation32::identity(),
            )?;
        } else if state.background_mode == 2 && !units.is_empty() && !request.prefix_only {
            let right = point
                .x
                .checked_add(shaped.advance.ceil() as i32)
                .ok_or(GdiError32::InvalidRequest)?;
            paint(
                Rect32 {
                    left: point.x.min(right),
                    top: baseline
                        .checked_sub(shaped.metrics.ascent)
                        .ok_or(GdiError32::InvalidRequest)?,
                    right: point.x.max(right),
                    bottom: baseline
                        .checked_add(shaped.metrics.descent)
                        .ok_or(GdiError32::InvalidRequest)?,
                },
                state.background_color,
                255,
                rotation,
            )?;
        }
        let mut cache: Vec<(u16, GlyphRaster32)> = Vec::new();
        cache
            .try_reserve(shaped.glyphs.len().min(256))
            .map_err(|_| GdiError32::Allocation)?;
        let mut raster_bytes = 0usize;
        for glyph in &shaped.glyphs {
            if request.prefix_only {
                continue;
            }
            let position =
                if let Some(position) = cache.iter().position(|entry| entry.0 == glyph.glyph) {
                    position
                } else {
                    let raster = self.fonts.rasterize(
                        &font,
                        glyph.glyph,
                        self.config.bitmap_bytes.saturating_sub(raster_bytes),
                    )?;
                    raster_bytes = raster_bytes
                        .checked_add(raster.coverage.len())
                        .ok_or(GdiError32::Capacity)?;
                    cache.try_reserve(1).map_err(|_| GdiError32::Allocation)?;
                    cache.push((glyph.glyph, raster));
                    cache.len() - 1
                };
            let raster = &cache[position].1;
            let left = point
                .x
                .checked_add(glyph.x.round() as i32)
                .and_then(|x| x.checked_add(raster.left))
                .ok_or(GdiError32::InvalidRequest)?;
            let top = baseline
                .checked_add(glyph.y.round() as i32)
                .and_then(|y| y.checked_add(raster.top))
                .ok_or(GdiError32::InvalidRequest)?;
            let glyph_bounds = rotation.device_bounds(
                &state,
                Rect32 {
                    left,
                    top,
                    right: left
                        .checked_add(raster.width as i32)
                        .ok_or(GdiError32::InvalidRequest)?,
                    bottom: top
                        .checked_add(raster.height as i32)
                        .ok_or(GdiError32::InvalidRequest)?,
                },
            )?;
            let visible = intersect(
                glyph_bounds,
                clip.map_or(bounds(layout), |clip| intersect(bounds(layout), clip)),
            );
            if visible.left >= visible.right || visible.top >= visible.bottom {
                continue;
            }
            for y in 0..raster.height {
                for x in 0..raster.width {
                    let coverage = raster.coverage[y * raster.width + x];
                    if coverage == 0 {
                        continue;
                    }
                    let x = left
                        .checked_add(x as i32)
                        .ok_or(GdiError32::InvalidRequest)?;
                    let y = top
                        .checked_add(y as i32)
                        .ok_or(GdiError32::InvalidRequest)?;
                    paint(
                        Rect32 {
                            left: x,
                            top: y,
                            right: x.checked_add(1).ok_or(GdiError32::InvalidRequest)?,
                            bottom: y.checked_add(1).ok_or(GdiError32::InvalidRequest)?,
                        },
                        state.text_color,
                        if font.quality == 3 {
                            if coverage >= 128 { 255 } else { 0 }
                        } else {
                            coverage
                        },
                        rotation,
                    )?;
                }
            }
        }
        let thickness = (shaped.metrics.height / 16).max(1);
        for glyph in shaped
            .glyphs
            .iter()
            .filter(|glyph| request.underlines.contains(&glyph.cluster))
        {
            let left = point
                .x
                .checked_add(glyph.x.round() as i32)
                .ok_or(GdiError32::InvalidRequest)?;
            let right = left
                .checked_add(glyph.advance.round() as i32)
                .ok_or(GdiError32::InvalidRequest)?;
            let y = baseline
                .checked_add((shaped.metrics.descent / 3).max(1))
                .ok_or(GdiError32::InvalidRequest)?;
            paint(
                Rect32 {
                    left,
                    top: y,
                    right,
                    bottom: y.checked_add(thickness).ok_or(GdiError32::InvalidRequest)?,
                },
                state.text_color,
                255,
                rotation,
            )?;
        }
        for y in [
            font.underline
                .then(|| baseline.checked_add((shaped.metrics.descent / 3).max(1)))
                .flatten(),
            font.strike_out
                .then(|| baseline.checked_sub(shaped.metrics.ascent / 3))
                .flatten(),
        ]
        .into_iter()
        .flatten()
        {
            paint(
                Rect32 {
                    left: point.x,
                    top: y,
                    right: point
                        .x
                        .checked_add(shaped.advance.round() as i32)
                        .ok_or(GdiError32::InvalidRequest)?,
                    bottom: y.checked_add(thickness).ok_or(GdiError32::InvalidRequest)?,
                },
                state.text_color,
                255,
                rotation,
            )?;
        }
        if state.text_align & 1 != 0 {
            state.current_position = rotation.advance(shaped.advance)?;
        }
        Ok(TextPixels32 {
            index,
            pixels,
            damage,
            state,
        })
    }
    pub(super) fn publish_text(
        &mut self,
        memory: &mut AddressSpace,
        dc: u32,
        prepared: TextPixels32,
        spans: &[(GuestAddress, &[u8])],
    ) -> Result<(), GdiError32> {
        self.commit_pixels_with_spans(
            memory,
            prepared.index,
            prepared.pixels,
            prepared.damage,
            spans,
        )?;
        self.model
            .set_state(dc, prepared.state)
            .expect("preflighted text DC");
        Ok(())
    }
    pub(in crate::process) fn draw_text_units(
        &mut self,
        memory: &mut AddressSpace,
        dc: u32,
        point: Point32,
        units: &[u16],
    ) -> Result<(), GdiError32> {
        if units.is_empty() {
            self.model.dc_kind(dc)?;
            return Ok(());
        }
        let prepared = self.prepare_text(
            memory,
            TextRequest32 {
                dc,
                point,
                units,
                options: 0,
                rectangle: None,
                underlines: &[],
                prefix_only: false,
                advances: None,
            },
        )?;
        self.publish_text(memory, dc, prepared, &[])
    }
}
fn override_advances(text: &mut ShapedText32, advances: &[(i32, i32)]) -> Result<(), GdiError32> {
    let mut pen = Point32::default();
    let mut natural = 0.0;
    for index in 0..text.glyphs.len() {
        let cluster = text.glyphs[index].cluster;
        let end = text
            .glyphs
            .iter()
            .filter_map(|glyph| (glyph.cluster > cluster).then_some(glyph.cluster))
            .min()
            .unwrap_or(advances.len());
        let is_last = text
            .glyphs
            .get(index + 1)
            .is_none_or(|glyph| glyph.cluster != cluster);
        let glyph = &mut text.glyphs[index];
        glyph.x += pen.x as f32 - natural;
        glyph.y += pen.y as f32;
        natural += glyph.advance;
        if is_last {
            for (x, y) in &advances[cluster..end] {
                pen.x = pen.x.checked_add(*x).ok_or(GdiError32::InvalidRequest)?;
                pen.y = pen.y.checked_add(*y).ok_or(GdiError32::InvalidRequest)?;
            }
        }
    }
    text.advance = pen.x as f32;
    Ok(())
}
pub(super) fn device_rect(state: &DcState32, rectangle: Rect32) -> Result<Rect32, GdiError32> {
    let a = state.device_point(Point32 {
        x: rectangle.left,
        y: rectangle.top,
    })?;
    let b = state.device_point(Point32 {
        x: rectangle.right,
        y: rectangle.bottom,
    })?;
    Ok(Rect32 {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    })
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
fn blend(background: u32, foreground: u32, coverage: u8) -> u32 {
    let coverage = u32::from(coverage);
    (0..3).fold(0, |color, index| {
        let shift = index * 8;
        let value = (((foreground >> shift) & 255) * coverage
            + ((background >> shift) & 255) * (255 - coverage)
            + 127)
            / 255;
        color | value << shift
    })
}
