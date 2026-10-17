use super::{super::context::GdiRuntime32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{DcState32, FontMetrics32, GdiApi32, GdiError32, ShapedText32},
        text::CodePage32,
    },
};
impl GdiRuntime32 {
    pub(in crate::process) fn font_metrics(
        &self,
        handle: u32,
    ) -> Result<FontMetrics32, GdiError32> {
        let descriptor = self.logical_font(handle)?;
        self.model
            .font_metric_snapshot(handle)?
            .map_or_else(|| self.fonts.resolve(descriptor), Ok)?
            .metrics()
    }

    pub(in crate::process) fn measure_text(
        &self,
        dc: u32,
        units: &[u16],
    ) -> Result<ShapedText32, GdiError32> {
        self.measure_text_for_state(self.model.state(dc)?, units)
    }
    pub(in crate::process::windows::gdi) fn measure_text_for_state(
        &self,
        state: &DcState32,
        units: &[u16],
    ) -> Result<ShapedText32, GdiError32> {
        let selected = self.resolved_state_metrics(state)?;
        let mut result = selected.shape(units, false, state.text_align & 0x100 != 0)?;
        apply_spacing(&mut result, state, units);
        if !result.advance.is_finite() || result.advance.abs() > i32::MAX as f32 {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(result)
    }
    pub(in crate::process::windows::gdi) fn font_query(
        &self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
        cp: CodePage32,
    ) -> Result<u32, GdiError32> {
        use GdiApi32::*;
        let state = self.model.state(args[0])?;
        let selected = self.resolved_dc_font(args[0])?;
        let wide = matches!(
            api,
            GetTextMetricsW
                | GetTextFaceW
                | GetTextExtentPoint32W
                | GetTextExtentExPointW
                | GetCharWidth32W
                | GetCharABCWidthsW
                | GetGlyphIndicesW
        );
        match api {
            GetTextMetricsA | GetTextMetricsW => {
                let bytes = metric_bytes(self.resolved_dc_metrics(args[0])?.metrics()?, wide, cp)?;
                memory.write(GuestAddress(args[1]), &bytes)?;
                Ok(1)
            }
            GetTextFaceA | GetTextFaceW => {
                let current = self.fonts.resolve(self.logical_font(state.font)?)?;
                let mut bytes = memory::encode(current.face_name(), wide, cp)?;
                let width = if wide { 2 } else { 1 };
                let required = bytes.len() / width + 1;
                if args[2] == 0 {
                    return Ok(required as u32);
                }
                if args[1] == 0 || args[1] > 32_768 {
                    return Err(GdiError32::InvalidRequest);
                }
                bytes.truncate((args[1] as usize - 1) * width);
                bytes.resize(bytes.len() + width, 0);
                memory.write(GuestAddress(args[2]), &bytes)?;
                Ok((bytes.len() / width) as u32)
            }
            GetTextExtentPoint32A
            | GetTextExtentPoint32W
            | GetTextExtentExPointA
            | GetTextExtentExPointW => {
                let units = memory::string(memory, args[1], wide, args[2] as i32, cp, 8192)?;
                let measured = self.measure_text(args[0], &units)?;
                let size = [measured.advance.round() as i32, measured.metrics.height]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>();
                if matches!(api, GetTextExtentPoint32A | GetTextExtentPoint32W) {
                    memory.write(GuestAddress(args[3]), &size)?;
                    return Ok(1);
                }
                let extents = cumulative(&measured, &units);
                let extents = transport_extents(&units, &extents, wide, cp, args[2] as usize)?;
                let fit = extents
                    .iter()
                    .take_while(|extent| **extent <= args[3] as i32)
                    .count() as u32;
                let fit_bytes = fit.to_le_bytes();
                let extent_bytes = extents
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>();
                let mut spans = vec![(GuestAddress(args[6]), size.as_slice())];
                if args[4] != 0 {
                    spans.push((GuestAddress(args[4]), &fit_bytes));
                }
                if args[5] != 0 && !extent_bytes.is_empty() {
                    spans.push((GuestAddress(args[5]), &extent_bytes));
                }
                memory.write_spans(&spans)?;
                Ok(1)
            }
            GetCharWidthA | GetCharWidth32A | GetCharWidth32W | GetCharABCWidthsA
            | GetCharABCWidthsW => {
                if args[2] < args[1] || args[2] - args[1] >= 8192 || args[2] > 0xffff {
                    return Err(GdiError32::InvalidRequest);
                }
                let mut output = Vec::new();
                output
                    .try_reserve_exact((args[2] - args[1] + 1) as usize * 12)
                    .map_err(|_| GdiError32::Allocation)?;
                for code in args[1]..=args[2] {
                    let character = if wide {
                        char::from_u32(code).unwrap_or('\u{fffd}')
                    } else {
                        let encoded = if code <= 255 {
                            vec![code as u8]
                        } else {
                            vec![(code >> 8) as u8, code as u8]
                        };
                        let units = cp
                            .decode(&encoded, false)
                            .map_err(|_| GdiError32::InvalidRequest)?;
                        char::decode_utf16(units)
                            .next()
                            .and_then(Result::ok)
                            .unwrap_or('\u{fffd}')
                    };
                    let glyph = selected.glyph_index(character);
                    let advance = selected.advance(glyph)?.round() as i32;
                    if matches!(api, GetCharABCWidthsA | GetCharABCWidthsW) {
                        let raster = selected.rasterize(glyph, self.config.bitmap_bytes)?;
                        for word in [
                            raster.left,
                            raster.width as i32,
                            advance - raster.left - raster.width as i32,
                        ] {
                            output.extend_from_slice(&word.to_le_bytes());
                        }
                    } else {
                        output.extend_from_slice(&advance.to_le_bytes());
                    }
                }
                memory.write(GuestAddress(args[3]), &output)?;
                Ok(1)
            }
            GetGlyphIndicesA | GetGlyphIndicesW => {
                if args[4] & !1 != 0 {
                    return Err(GdiError32::InvalidRequest);
                }
                let units = memory::string(memory, args[1], wide, args[2] as i32, cp, 8192)?;
                let mut indices = Vec::new();
                indices
                    .try_reserve_exact(units.len() * 2)
                    .map_err(|_| GdiError32::Allocation)?;
                for unit in units.iter().copied() {
                    let glyph =
                        selected.glyph_index(char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}'));
                    indices.extend_from_slice(
                        &(if glyph == 0 && args[4] & 1 != 0 {
                            u16::MAX
                        } else {
                            glyph
                        })
                        .to_le_bytes(),
                    );
                }
                if !indices.is_empty() {
                    memory.write(GuestAddress(args[3]), &indices)?;
                }
                Ok(units.len() as u32)
            }
            _ => Err(GdiError32::InvalidRequest),
        }
    }
}
pub(super) fn apply_spacing(text: &mut ShapedText32, state: &DcState32, units: &[u16]) {
    let mut added = 0.0;
    let per_break = if state.justification_breaks != 0 {
        state.justification_extra as f32 / state.justification_breaks as f32
    } else {
        0.0
    };
    for glyph in &mut text.glyphs {
        glyph.x += added;
        let extra = state.character_extra as f32
            + if units.get(glyph.cluster) == Some(&32) {
                per_break
            } else {
                0.0
            };
        glyph.advance += extra;
        added += extra;
    }
    text.advance += added;
}
fn cumulative(text: &ShapedText32, units: &[u16]) -> Vec<i32> {
    let mut widths = vec![0.0; units.len()];
    for glyph in &text.glyphs {
        if let Some(width) = widths.get_mut(glyph.cluster) {
            *width += glyph.advance;
        }
    }
    let mut total = 0.0;
    widths
        .into_iter()
        .map(|width| {
            total += width;
            total.round() as i32
        })
        .collect()
}
fn transport_extents(
    units: &[u16],
    extents: &[i32],
    wide: bool,
    cp: CodePage32,
    count: usize,
) -> Result<Vec<i32>, GdiError32> {
    if wide {
        return Ok(extents.to_vec());
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| GdiError32::Allocation)?;
    let mut at = 0;
    for character in char::decode_utf16(units.iter().copied()) {
        let character = character.unwrap_or('\u{fffd}');
        let mut buffer = [0; 2];
        let encoded = character.encode_utf16(&mut buffer);
        at += encoded.len();
        let bytes = cp
            .encode_windows(encoded, 0, b'?')
            .map_err(|_| GdiError32::InvalidRequest)?
            .0;
        output.extend(std::iter::repeat_n(extents[at - 1], bytes.len()));
    }
    if output.len() != count {
        return Err(GdiError32::InvalidRequest);
    }
    Ok(output)
}
pub(super) fn metric_bytes(
    metrics: FontMetrics32,
    wide: bool,
    cp: CodePage32,
) -> Result<Vec<u8>, GdiError32> {
    let mut bytes = vec![0; if wide { 60 } else { 56 }];
    for (output, word) in bytes[..44].chunks_exact_mut(4).zip([
        metrics.height,
        metrics.ascent,
        metrics.descent,
        metrics.internal_leading,
        metrics.external_leading,
        metrics.average_width,
        metrics.maximum_width,
        metrics.weight,
        0,
        96,
        96,
    ]) {
        output.copy_from_slice(&word.to_le_bytes());
    }
    let chars = [
        metrics.first,
        metrics.last,
        metrics.default,
        metrics.break_character,
    ];
    let style = 44 + if wide { 8 } else { 4 };
    if wide {
        for (output, unit) in bytes[44..52].chunks_exact_mut(2).zip(chars) {
            output.copy_from_slice(&unit.to_le_bytes());
        }
    } else {
        for (output, unit) in bytes[44..48].iter_mut().zip(chars) {
            *output = cp
                .encode_windows(&[unit], 0, b'?')
                .map_err(|_| GdiError32::InvalidRequest)?
                .0[0];
        }
    }
    bytes[style..style + 5].copy_from_slice(&[
        u8::from(metrics.italic),
        u8::from(metrics.underline),
        u8::from(metrics.strike_out),
        6 | u8::from(!metrics.fixed_pitch),
        metrics.charset,
    ]);
    Ok(bytes)
}
