use super::{
    super::{context::GdiRuntime32, memory as gdi_memory},
    memory,
    rendering::{TextPixels32, TextRequest32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiApi32, GdiError32},
        text::CodePage32,
        user32::{Point32, Rect32},
    },
};
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn draw_text_call(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
        cp: CodePage32,
    ) -> Result<u32, GdiError32> {
        let wide = matches!(api, GdiApi32::DrawTextW | GdiApi32::DrawTextExW);
        let extended = matches!(api, GdiApi32::DrawTextExA | GdiApi32::DrawTextExW);
        let units = memory::string(memory, args[1], wide, args[2] as i32, cp, 8192)?;
        let mut rectangle = gdi_memory::rect(memory, args[3])?;
        let mut params = [20, 8, 0, 0, 0];
        if extended && args[5] != 0 {
            params = gdi_memory::words(memory, args[5])?;
            if params[0] != 20 {
                return Err(GdiError32::InvalidRequest);
            }
        }
        if params[2] > i32::MAX as u32 || params[3] > i32::MAX as u32 {
            return Err(GdiError32::InvalidRequest);
        }
        let mut flags = args[4];
        let mut tab_length = if params[1] == 0 {
            8
        } else {
            params[1] as usize
        };
        if !extended && flags & 0x80 != 0 {
            tab_length = ((flags >> 8) & 255) as usize;
            if tab_length == 0 {
                tab_length = 8;
            }
            flags &= !0xff00;
        }
        if flags & 0x1000 != 0 {
            return Err(GdiError32::Unsupported);
        }
        if flags & 0x40 != 0 && flags & (0x4000 | 0x8000 | 0x40000) != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let modify = flags & 0x10000 != 0 && flags & (0x4000 | 0x8000) != 0;
        if modify && flags & 0x20 == 0 {
            return Err(GdiError32::Unsupported);
        }
        let average = self.measure_text(args[0], &[])?.metrics.average_width;
        let left_margin = average
            .checked_mul(params[2] as i32)
            .ok_or(GdiError32::InvalidRequest)?;
        let right_margin = average
            .checked_mul(params[3] as i32)
            .ok_or(GdiError32::InvalidRequest)?;
        let bounds = Rect32 {
            left: rectangle
                .left
                .checked_add(left_margin)
                .ok_or(GdiError32::InvalidRequest)?,
            right: rectangle
                .right
                .checked_sub(right_margin)
                .ok_or(GdiError32::InvalidRequest)?,
            ..rectangle
        };
        let paragraph = self.prepare_paragraph(args[0], &units, bounds, flags, tab_length)?;
        let rect_bytes;
        let modified_bytes;
        let length_bytes = (if wide {
            units.len()
        } else {
            memory::encode(&units, false, cp)?.len()
        } as u32)
            .to_le_bytes();
        let mut spans: Vec<(GuestAddress, &[u8])> = Vec::new();
        if modify {
            let modified = paragraph
                .lines
                .first()
                .map_or(&[][..], |line| line.units.as_slice());
            modified_bytes = {
                let mut bytes = memory::encode(modified, wide, cp)?;
                bytes.resize(bytes.len() + if wide { 2 } else { 1 }, 0);
                bytes
            };
            spans.push((GuestAddress(args[1]), &modified_bytes));
        }
        if flags & 0x400 != 0 {
            rectangle.right = rectangle
                .left
                .checked_add(paragraph.width)
                .and_then(|right| {
                    right
                        .checked_add(left_margin)
                        .and_then(|right| right.checked_add(right_margin))
                })
                .ok_or(GdiError32::InvalidRequest)?;
            rectangle.bottom = rectangle
                .top
                .checked_add(paragraph.height)
                .ok_or(GdiError32::InvalidRequest)?;
            rect_bytes = [
                rectangle.left,
                rectangle.top,
                rectangle.right,
                rectangle.bottom,
            ]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
            spans.push((GuestAddress(args[3]), &rect_bytes));
        }
        if extended && args[5] != 0 {
            spans.push((
                GuestAddress(args[5].checked_add(16).ok_or(GdiError32::InvalidRequest)?),
                &length_bytes,
            ));
        }
        if flags & 0x400 != 0 || paragraph.lines.is_empty() {
            if !spans.is_empty() {
                memory.write_spans(&spans)?;
            }
            return Ok(paragraph.height as u32);
        }
        let original = self.model.state(args[0])?.clone();
        let mut state = original.clone();
        state.text_align = if flags & 0x20000 != 0 { 256 } else { 0 };
        let bounds_height = bounds
            .bottom
            .checked_sub(bounds.top)
            .ok_or(GdiError32::InvalidRequest)?;
        let mut y = bounds.top;
        if flags & 0x20 != 0 {
            if flags & 4 != 0 {
                y = y
                    .checked_add(
                        bounds_height
                            .checked_sub(paragraph.height)
                            .ok_or(GdiError32::InvalidRequest)?
                            / 2,
                    )
                    .ok_or(GdiError32::InvalidRequest)?;
            } else if flags & 8 != 0 {
                y = bounds
                    .bottom
                    .checked_sub(paragraph.height)
                    .ok_or(GdiError32::InvalidRequest)?;
            }
        }
        let mut prepared: Option<TextPixels32> = None;
        let start_y = y;
        for line in &paragraph.lines {
            if flags & 0x2000 != 0
                && y.checked_add(paragraph.line_height)
                    .is_none_or(|bottom| bottom > bounds.bottom)
            {
                break;
            }
            let x = if flags & 2 != 0 {
                bounds
                    .right
                    .checked_sub(line.width)
                    .ok_or(GdiError32::InvalidRequest)?
            } else if flags & 1 != 0 {
                bounds
                    .left
                    .checked_add(
                        bounds
                            .right
                            .checked_sub(bounds.left)
                            .and_then(|width| width.checked_sub(line.width))
                            .ok_or(GdiError32::InvalidRequest)?
                            / 2,
                    )
                    .ok_or(GdiError32::InvalidRequest)?
            } else {
                bounds.left
            };
            prepared = Some(self.prepare_text_from(
                memory,
                TextRequest32 {
                    dc: args[0],
                    point: Point32 { x, y },
                    units: &line.units,
                    options: if flags & 0x100 == 0 { 4 } else { 0 },
                    rectangle: Some(bounds),
                    underlines: &line.underlines,
                    prefix_only: flags & 0x200000 != 0,
                    advances: line.advances.as_deref(),
                },
                state.clone(),
                prepared,
            )?);
            y = y
                .checked_add(paragraph.line_height)
                .ok_or(GdiError32::InvalidRequest)?;
        }
        let Some(mut prepared) = prepared else {
            if !spans.is_empty() {
                memory.write_spans(&spans)?;
            }
            return Ok(0);
        };
        prepared.state = original;
        self.publish_text(memory, args[0], prepared, &spans)?;
        Ok((if flags & 0x20 != 0 && flags & (4 | 8) != 0 {
            start_y
                .checked_add(paragraph.height)
                .and_then(|bottom| bottom.checked_sub(rectangle.top))
                .ok_or(GdiError32::InvalidRequest)?
        } else {
            paragraph.height
        }) as u32)
    }
}
