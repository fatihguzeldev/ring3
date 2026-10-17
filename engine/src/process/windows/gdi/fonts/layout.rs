use super::super::context::GdiRuntime32;
use crate::windows::{gdi::GdiError32, user32::Rect32};

pub(in crate::process::windows::gdi) struct TextLine32 {
    pub units: Vec<u16>,
    pub underlines: Vec<usize>,
    pub width: i32,
    pub advances: Option<Vec<(i32, i32)>>,
}
pub(in crate::process::windows::gdi) struct Paragraph32 {
    pub lines: Vec<TextLine32>,
    pub width: i32,
    pub height: i32,
    pub line_height: i32,
}
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn prepare_paragraph(
        &self,
        dc: u32,
        units: &[u16],
        rectangle: Rect32,
        flags: u32,
        tab_length: usize,
    ) -> Result<Paragraph32, GdiError32> {
        self.prepare_paragraph_for_state(self.model.state(dc)?, units, rectangle, flags, tab_length)
    }
    pub(in crate::process::windows::gdi) fn prepare_paragraph_for_state(
        &self,
        state: &crate::windows::gdi::DcState32,
        units: &[u16],
        rectangle: Rect32,
        flags: u32,
        tab_length: usize,
    ) -> Result<Paragraph32, GdiError32> {
        if flags & !(0x000f_ffff | 0x100000 | 0x200000) != 0
            || units.len() > 8192
            || tab_length == 0
            || tab_length > 256
        {
            return Err(GdiError32::InvalidRequest);
        }
        let metrics = self.resolved_state_metrics(state)?.metrics()?;
        let line_height = metrics
            .height
            .checked_add(if flags & 0x200 != 0 {
                metrics.external_leading
            } else {
                0
            })
            .ok_or(GdiError32::InvalidRequest)?;
        let width = rectangle
            .right
            .checked_sub(rectangle.left)
            .ok_or(GdiError32::InvalidRequest)?
            .max(0);
        let single = flags & 0x20 != 0;
        let mut processed = Vec::new();
        let mut prefixes = Vec::new();
        processed
            .try_reserve(units.len())
            .map_err(|_| GdiError32::Allocation)?;
        prefixes
            .try_reserve(units.len())
            .map_err(|_| GdiError32::Allocation)?;
        let mut index = 0;
        while index < units.len() {
            let unit = units[index];
            if flags & 0x800 == 0 && unit == u16::from(b'&') {
                if units.get(index + 1) == Some(&u16::from(b'&')) {
                    processed.push(unit);
                    index += 2;
                    continue;
                }
                if index + 1 < units.len() && flags & 0x100000 == 0 {
                    prefixes.push(processed.len());
                }
                index += 1;
                continue;
            }
            if single && matches!(unit, 10 | 13) {
                if unit != 13 || units.get(index + 1) != Some(&10) {
                    processed.push(32);
                }
            } else {
                processed.push(unit);
            }
            index += 1;
        }
        let mut lines = Vec::new();
        lines.try_reserve(16).map_err(|_| GdiError32::Allocation)?;
        let mut start = 0;
        while start <= processed.len() {
            let end = if single {
                processed.len()
            } else {
                processed[start..]
                    .iter()
                    .position(|unit| matches!(*unit, 10 | 13))
                    .map_or(processed.len(), |offset| start + offset)
            };
            let mut at = start;
            loop {
                let mut stop = end;
                if flags & 0x10 != 0
                    && !single
                    && self
                        .measure_text_for_state(state, &processed[at..end])?
                        .advance
                        .round() as i32
                        > width
                {
                    stop = largest_prefix(self, state, &processed, at, end, width)?;
                    if let Some(space) = processed[at..stop]
                        .iter()
                        .rposition(|unit| matches!(*unit, 9 | 32))
                    {
                        if space != 0 {
                            stop = at + space;
                        }
                    }
                    if stop == at {
                        stop = next_character(&processed, at, end);
                    }
                    if !processed[at..stop]
                        .iter()
                        .any(|unit| matches!(*unit, 9 | 32))
                        && stop < end
                        && !processed[at..stop].iter().any(|unit| *unit >= 0x2e80)
                    {
                        stop = processed[stop..end]
                            .iter()
                            .position(|unit| matches!(*unit, 9 | 32))
                            .map_or(end, |offset| stop + offset);
                    }
                }
                let mut line_units = processed[at..stop].to_vec();
                let mut underlines = prefixes
                    .iter()
                    .copied()
                    .filter(|position| *position >= at && *position < stop)
                    .map(|position| position - at)
                    .collect::<Vec<_>>();
                if flags & (0x4000 | 0x8000 | 0x40000) != 0
                    && self
                        .measure_text_for_state(state, &line_units)?
                        .advance
                        .round() as i32
                        > width
                {
                    ellipsize(self, state, &mut line_units, width, flags)?;
                    underlines.retain(|index| *index < line_units.len());
                }
                let advances = if flags & 0x40 != 0 {
                    expand_tabs(self, state, &mut line_units, tab_length)?
                } else {
                    None
                };
                let line_width = if let Some(advances) = &advances {
                    advances
                        .iter()
                        .try_fold(0i32, |total, advance| total.checked_add(advance.0))
                        .ok_or(GdiError32::InvalidRequest)?
                } else {
                    self.measure_text_for_state(state, &line_units)?
                        .advance
                        .round() as i32
                };
                lines.try_reserve(1).map_err(|_| GdiError32::Allocation)?;
                lines.push(TextLine32 {
                    units: line_units,
                    underlines,
                    width: line_width,
                    advances,
                });
                if stop == end {
                    break;
                }
                at = stop;
                while at < end && matches!(processed[at], 9 | 32) {
                    at += 1;
                }
            }
            if end == processed.len() {
                break;
            }
            start = end + 1;
            if processed[end] == 13 && processed.get(start) == Some(&10) {
                start += 1;
            }
        }
        if units.is_empty() {
            lines.clear();
        }
        let width = lines.iter().map(|line| line.width).max().unwrap_or(0);
        let height = line_height
            .checked_mul(lines.len() as i32)
            .ok_or(GdiError32::InvalidRequest)?;
        Ok(Paragraph32 {
            lines,
            width,
            height,
            line_height,
        })
    }
}
fn next_character(units: &[u16], at: usize, end: usize) -> usize {
    if (0xd800..=0xdbff).contains(&units[at])
        && at + 1 < end
        && (0xdc00..=0xdfff).contains(&units[at + 1])
    {
        at + 2
    } else {
        at + 1
    }
}
fn largest_prefix(
    runtime: &GdiRuntime32,
    state: &crate::windows::gdi::DcState32,
    units: &[u16],
    start: usize,
    end: usize,
    width: i32,
) -> Result<usize, GdiError32> {
    let mut positions = Vec::new();
    let mut at = start;
    positions
        .try_reserve(end - start)
        .map_err(|_| GdiError32::Allocation)?;
    while at < end {
        at = next_character(units, at, end);
        positions.push(at);
    }
    let mut low = 0;
    let mut high = positions.len();
    while low < high {
        let mid = (low + high) / 2;
        if runtime
            .measure_text_for_state(state, &units[start..positions[mid]])?
            .advance
            .round() as i32
            <= width
        {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Ok(if low == 0 { start } else { positions[low - 1] })
}
fn expand_tabs(
    runtime: &GdiRuntime32,
    state: &crate::windows::gdi::DcState32,
    units: &mut [u16],
    tab_length: usize,
) -> Result<Option<Vec<(i32, i32)>>, GdiError32> {
    if !units.contains(&9) {
        return Ok(None);
    }
    let tabs = units.iter().map(|unit| *unit == 9).collect::<Vec<_>>();
    for unit in units.iter_mut() {
        if *unit == 9 {
            *unit = 32;
        }
    }
    let measured = runtime.measure_text_for_state(state, units)?;
    let stop = measured
        .metrics
        .average_width
        .checked_mul(tab_length as i32)
        .filter(|value| *value > 0)
        .ok_or(GdiError32::InvalidRequest)?;
    let mut natural = vec![0.0; units.len()];
    for glyph in &measured.glyphs {
        if let Some(advance) = natural.get_mut(glyph.cluster) {
            *advance += glyph.advance;
        }
    }
    let mut output = Vec::new();
    output
        .try_reserve(units.len())
        .map_err(|_| GdiError32::Allocation)?;
    let mut x = 0i32;
    let mut fraction = 0.0f32;
    for (index, advance) in natural.into_iter().enumerate() {
        let width = if tabs[index] {
            fraction = 0.0;
            stop - x.rem_euclid(stop)
        } else {
            let next = fraction + advance;
            let width = next.round() as i32 - fraction.round() as i32;
            fraction = next;
            width
        };
        x = x.checked_add(width).ok_or(GdiError32::InvalidRequest)?;
        output.push((width, 0));
    }
    Ok(Some(output))
}

fn ellipsize(
    runtime: &GdiRuntime32,
    state: &crate::windows::gdi::DcState32,
    units: &mut Vec<u16>,
    width: i32,
    flags: u32,
) -> Result<(), GdiError32> {
    let ellipsis = [46u16; 3];
    let remaining = width
        - runtime
            .measure_text_for_state(state, &ellipsis)?
            .advance
            .round() as i32;
    if remaining <= 0 {
        *units = ellipsis.to_vec();
        return Ok(());
    }
    if flags & 0x4000 != 0 {
        if let Some(separator) = units.iter().rposition(|unit| matches!(*unit, 47 | 92)) {
            let suffix = units[separator..].to_vec();
            let tail_width = runtime
                .measure_text_for_state(state, &suffix)?
                .advance
                .round() as i32;
            if tail_width <= remaining {
                let stop =
                    largest_prefix(runtime, state, units, 0, separator, remaining - tail_width)?;
                units.truncate(stop);
                units.extend_from_slice(&ellipsis);
                units.extend(suffix);
                return Ok(());
            }
        }
    }
    let mut stop = largest_prefix(runtime, state, units, 0, units.len(), remaining)?;
    if flags & 0x40000 != 0 {
        stop = units[..stop]
            .iter()
            .rposition(|unit| matches!(*unit, 9 | 32))
            .unwrap_or(stop);
    }
    units.truncate(stop);
    units.extend_from_slice(&ellipsis);
    Ok(())
}
