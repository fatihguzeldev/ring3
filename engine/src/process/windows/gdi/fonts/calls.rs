use super::{super::context::GdiRuntime32, memory, rendering::TextRequest32};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiApi32, GdiError32},
        text::CodePage32,
        user32::Point32,
    },
};
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn text_call(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
        cp: CodePage32,
    ) -> Result<u32, GdiError32> {
        use GdiApi32::*;
        if matches!(
            api,
            GetTextAlign
                | SetTextAlign
                | GetTextCharacterExtra
                | SetTextCharacterExtra
                | SetTextJustification
        ) {
            let mut state = self.model.state(args[0])?.clone();
            let value = match api {
                GetTextAlign => return Ok(state.text_align),
                SetTextAlign => {
                    if args[1] & !0x11f != 0 || args[1] & 6 == 4 || args[1] & 24 == 16 {
                        return Err(GdiError32::InvalidRequest);
                    }
                    let old = state.text_align;
                    state.text_align = args[1];
                    old
                }
                GetTextCharacterExtra => return Ok(state.character_extra as u32),
                SetTextCharacterExtra => {
                    if args[1] == 0x80000000 {
                        return Err(GdiError32::InvalidRequest);
                    }
                    let old = state.character_extra;
                    state.character_extra = args[1] as i32;
                    old as u32
                }
                SetTextJustification => {
                    if (args[2] as i32) < 0 {
                        return Err(GdiError32::InvalidRequest);
                    }
                    state.justification_extra = args[1] as i32;
                    state.justification_breaks = args[2];
                    1
                }
                _ => unreachable!(),
            };
            self.model.set_state(args[0], state)?;
            return Ok(value);
        }
        let extended = matches!(api, ExtTextOutA | ExtTextOutW);
        let wide = matches!(api, TextOutW | ExtTextOutW);
        let (options, pointer, count) = if extended {
            (args[3], args[5], args[6])
        } else {
            (0, args[3], args[4])
        };
        let glyph_indices = options & 16 != 0;
        if glyph_indices && !wide {
            return Err(GdiError32::InvalidRequest);
        }
        let units = memory::string(memory, pointer, wide, count as i32, cp, 8192)?;
        let rectangle = if extended && args[4] != 0 {
            Some(super::super::memory::rect(memory, args[4])?)
        } else {
            None
        };
        let advances = if extended && args[7] != 0 {
            let pair = options & 8192 != 0;
            let mut bytes =
                super::super::memory::zeroes(count as usize * if pair { 8 } else { 4 })?;
            memory.read(GuestAddress(args[7]), &mut bytes)?;
            let raw = bytes
                .chunks_exact(if pair { 8 } else { 4 })
                .map(|bytes| {
                    (
                        i32::from_le_bytes(bytes[..4].try_into().unwrap()),
                        if pair {
                            i32::from_le_bytes(bytes[4..8].try_into().unwrap())
                        } else {
                            0
                        },
                    )
                })
                .collect::<Vec<_>>();
            if units.len() != raw.len() {
                return Err(GdiError32::Unsupported);
            }
            Some(raw)
        } else {
            None
        };
        if units.is_empty() && options & 2 == 0 {
            self.model.dc_kind(args[0])?;
            return Ok(1);
        }
        if !extended {
            self.draw_text_units(
                memory,
                args[0],
                Point32 {
                    x: args[1] as i32,
                    y: args[2] as i32,
                },
                &units,
            )?;
            return Ok(1);
        }
        let prepared = self.prepare_text(
            memory,
            TextRequest32 {
                dc: args[0],
                point: Point32 {
                    x: args[1] as i32,
                    y: args[2] as i32,
                },
                units: &units,
                options,
                rectangle,
                underlines: &[],
                prefix_only: false,
                advances: advances.as_deref(),
            },
        )?;
        self.publish_text(memory, args[0], prepared, &[])?;
        Ok(1)
    }
}
