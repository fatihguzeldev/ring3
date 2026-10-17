use super::super::{User32Context32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::gdi::fonts::memory as fonts,
    windows::{
        gdi::GdiError32,
        imm32::{CandidateWindow32, CompositionWindow32, ImmApi32 as Api},
        user32::{Point32, Rect32, User32Error32 as Error},
    },
};

impl User32Context32 {
    pub(super) fn imm_preferences(
        &mut self,
        api: Api,
        args: &[u32],
        thread: u32,
        code_page: crate::windows::text::CodePage32,
        ram: &mut AddressSpace,
    ) -> Result<Option<u32>, Error> {
        let query = matches!(
            api,
            Api::ImmGetCompositionStringA | Api::ImmGetCompositionStringW
        );
        if query {
            let Ok(context) = self.ime.context(args[0]) else {
                return Ok(Some((-2i32) as u32));
            };
            let cp = code_page;
            let offsets =
                if api == Api::ImmGetCompositionStringA && matches!(args[1], 0x10 | 0x20 | 0x80) {
                    Some(ansi_offsets(&context.composition.text, cp)?)
                } else {
                    None
                };
            let bytes = match args[1] {
                8 | 0x800 => {
                    let text = if args[1] == 8 {
                        &context.composition.text
                    } else {
                        &context.composition.result
                    };
                    if api == Api::ImmGetCompositionStringW {
                        text.iter()
                            .flat_map(|unit| unit.to_le_bytes())
                            .collect::<Vec<_>>()
                    } else {
                        cp.encode_windows(text, 0, b'?')
                            .map_err(|_| Error::Win32(1113))?
                            .0
                    }
                }
                0x10 if offsets.is_some() => {
                    let offsets = offsets.as_ref().unwrap();
                    let mut attributes = Vec::new();
                    attributes
                        .try_reserve_exact(*offsets.last().unwrap() as usize)
                        .map_err(|_| Error::Allocation)?;
                    for (index, &attribute) in context.composition.attributes.iter().enumerate() {
                        attributes.extend(std::iter::repeat_n(
                            attribute,
                            (offsets[index + 1] - offsets[index]) as usize,
                        ));
                    }
                    attributes
                }
                0x10 => context.composition.attributes.clone(),
                0x20 => context
                    .composition
                    .clauses
                    .iter()
                    .flat_map(|&value| {
                        offsets
                            .as_ref()
                            .map_or(value, |offsets| offsets[value as usize])
                            .to_le_bytes()
                    })
                    .collect(),
                0x80 => {
                    return Ok(Some(
                        offsets
                            .as_ref()
                            .map_or(context.composition.cursor, |offsets| {
                                offsets[context.composition.cursor as usize]
                            }),
                    ));
                }
                0x100 => return Ok(Some(0)),
                _ => return Ok(Some((-2i32) as u32)),
            };
            if args[3] == 0 {
                return Ok(Some(bytes.len() as u32));
            }
            let mut count = bytes.len().min(args[3] as usize);
            if api == Api::ImmGetCompositionStringW && matches!(args[1], 8 | 0x800) {
                count &= !1;
            }
            ram.write(GuestAddress(args[2]), &bytes[..count])?;
            return Ok(Some(count as u32));
        }
        if !matches!(
            api,
            Api::ImmGetCompositionWindow
                | Api::ImmSetCompositionWindow
                | Api::ImmGetCandidateWindow
                | Api::ImmSetCandidateWindow
                | Api::ImmGetCompositionFontA
                | Api::ImmGetCompositionFontW
                | Api::ImmSetCompositionFontA
                | Api::ImmSetCompositionFontW
        ) {
            return Ok(None);
        }
        if self.ime.context(args[0]).is_err() {
            return Ok(Some(0));
        }
        match api {
            Api::ImmGetCompositionWindow => {
                let Some(value) = self.ime.context(args[0]).unwrap().composition_window else {
                    return Ok(Some(0));
                };
                write_words(ram, args[1], &form(value))?;
            }
            Api::ImmSetCompositionWindow => {
                let value = composition_form(memory::words(ram, args[1])?);
                if value.style & !0x23 != 0 {
                    return Ok(Some(0));
                }
                let Ok(context) = self.ime.context_mut(args[0], thread) else {
                    return Ok(Some(0));
                };
                context.composition_window = Some(value);
            }
            Api::ImmGetCandidateWindow => {
                let Some(value) = self
                    .ime
                    .context(args[0])
                    .unwrap()
                    .candidate_windows
                    .get(args[1] as usize)
                    .copied()
                    .flatten()
                else {
                    return Ok(Some(0));
                };
                let mut words = vec![value.index];
                words.extend(form(CompositionWindow32 {
                    style: value.style,
                    position: value.position,
                    area: value.area,
                }));
                write_words(ram, args[2], &words)?;
            }
            Api::ImmSetCandidateWindow => {
                let [index, style, x, y, left, top, right, bottom] = memory::words(ram, args[1])?;
                if index >= 4 || style & !0xc0 != 0 {
                    return Ok(Some(0));
                }
                let Ok(context) = self.ime.context_mut(args[0], thread) else {
                    return Ok(Some(0));
                };
                context.candidate_windows[index as usize] = Some(CandidateWindow32 {
                    index,
                    style,
                    position: Point32 {
                        x: x as i32,
                        y: y as i32,
                    },
                    area: Rect32 {
                        left: left as i32,
                        top: top as i32,
                        right: right as i32,
                        bottom: bottom as i32,
                    },
                });
            }
            Api::ImmGetCompositionFontA | Api::ImmGetCompositionFontW => {
                let bytes = fonts::encode_font(
                    &self.ime.context(args[0]).unwrap().font,
                    api == Api::ImmGetCompositionFontW,
                    code_page,
                )
                .map_err(font_error)?;
                ram.write(GuestAddress(args[1]), &bytes)?;
            }
            Api::ImmSetCompositionFontA | Api::ImmSetCompositionFontW => {
                let font = fonts::logical_font(
                    ram,
                    args[1],
                    api == Api::ImmSetCompositionFontW,
                    code_page,
                )
                .map_err(font_error)?;
                if font.check().is_err() {
                    return Ok(Some(0));
                }
                let Ok(context) = self.ime.context_mut(args[0], thread) else {
                    return Ok(Some(0));
                };
                context.font = font;
            }
            _ => unreachable!(),
        }
        Ok(Some(1))
    }
}

fn composition_form([style, x, y, left, top, right, bottom]: [u32; 7]) -> CompositionWindow32 {
    CompositionWindow32 {
        style,
        position: Point32 {
            x: x as i32,
            y: y as i32,
        },
        area: Rect32 {
            left: left as i32,
            top: top as i32,
            right: right as i32,
            bottom: bottom as i32,
        },
    }
}
fn form(value: CompositionWindow32) -> [u32; 7] {
    [
        value.style,
        value.position.x as u32,
        value.position.y as u32,
        value.area.left as u32,
        value.area.top as u32,
        value.area.right as u32,
        value.area.bottom as u32,
    ]
}
fn write_words(ram: &mut AddressSpace, address: u32, words: &[u32]) -> Result<(), Error> {
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    ram.write(GuestAddress(address), &bytes)?;
    Ok(())
}
fn font_error(error: GdiError32) -> Error {
    match error {
        GdiError32::Memory(error) => Error::Memory(error),
        GdiError32::Allocation => Error::Allocation,
        _ => Error::Win32(87),
    }
}

fn ansi_offsets(
    units: &[u16],
    code_page: crate::windows::text::CodePage32,
) -> Result<Vec<u32>, Error> {
    let mut offsets = Vec::new();
    offsets
        .try_reserve_exact(units.len() + 1)
        .map_err(|_| Error::Allocation)?;
    offsets.push(0);
    let mut at = 0;
    let mut bytes = 0;
    while at < units.len() {
        let count = if (0xd800..=0xdbff).contains(&units[at])
            && units
                .get(at + 1)
                .is_some_and(|value| (0xdc00..=0xdfff).contains(value))
        {
            2
        } else {
            1
        };
        bytes += code_page
            .encode_windows(&units[at..at + count], 0, b'?')
            .map_err(|_| Error::Win32(1113))?
            .0
            .len() as u32;
        offsets.extend(std::iter::repeat_n(bytes, count));
        at += count;
    }
    Ok(offsets)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ansi_composition_attributes_clauses_and_cursor_follow_multibyte_character_offsets() {
        let (mut context, mut ram, call) = crate::process::windows::user32::tests::fixture();
        let handle = context.ime.create_context(call.thread_id).unwrap();
        context.ime.set_composition(handle, call.thread_id, crate::windows::imm32::InputComposition32 {
            text: vec![0x3042, 65], attributes: vec![7, 9], clauses: vec![0, 1, 2], cursor: 1, ..Default::default()
        }).unwrap();
        let cp = crate::windows::text::CodePage32::from_id(932).unwrap();
        let query = |context: &mut User32Context32, ram: &mut AddressSpace, index| context.complete_imm32_call(Api::ImmGetCompositionStringA, &[handle, index, 0x11000, 32], call.thread_id, cp, ram).unwrap();
        assert_eq!(query(&mut context, &mut ram, 8), 3);
        let mut bytes = [0;3];ram.read(GuestAddress(0x11000), &mut bytes).unwrap();assert_eq!(bytes, [0x82,0xa0,65]);
        assert_eq!(query(&mut context, &mut ram, 0x10), 3);
        ram.read(GuestAddress(0x11000), &mut bytes).unwrap();assert_eq!(bytes, [7,7,9]);
        assert_eq!(query(&mut context, &mut ram, 0x20), 12);
        assert_eq!(memory::words::<3>(&ram, 0x11000).unwrap(), [0,2,3]);
        assert_eq!(query(&mut context, &mut ram, 0x80), 2);
    }

    #[test]
    fn composition_queries_count_bytes_preserve_adjacent_output_and_do_not_add_terminators() {
        let (mut c, mut ram, call) = crate::process::windows::user32::tests::fixture();
        let h = c.ime.create_context(call.thread_id).unwrap();
        c.ime
            .set_composition(
                h,
                call.thread_id,
                crate::windows::imm32::InputComposition32 {
                    text: vec![65, 0x00df],
                    result: vec![66],
                    cursor: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        let at = 0x11000;
        assert_eq!(
            c.complete_imm32_call(
                Api::ImmGetCompositionStringW,
                &[h, 8, 0, 0],
                call.thread_id,
                call.ansi,
                &mut ram
            )
            .unwrap(),
            4
        );
        ram.write(GuestAddress(at), &[0xcc; 8]).unwrap();
        assert_eq!(
            c.complete_imm32_call(
                Api::ImmGetCompositionStringW,
                &[h, 8, at, 3],
                call.thread_id,
                call.ansi,
                &mut ram
            )
            .unwrap(),
            2
        );
        let mut value = [0; 4];
        ram.read(GuestAddress(at), &mut value).unwrap();
        assert_eq!(value, [65, 0, 0xcc, 0xcc]);
        assert_eq!(
            c.complete_imm32_call(
                Api::ImmGetCompositionStringA,
                &[h, 8, at, 8],
                call.thread_id,
                call.ansi,
                &mut ram
            )
            .unwrap(),
            2
        );
        assert_eq!(
            c.complete_imm32_call(
                Api::ImmGetCompositionStringW,
                &[h, 0x800, at, 8],
                call.thread_id,
                call.ansi,
                &mut ram
            )
            .unwrap(),
            2
        );
        assert_eq!(
            c.complete_imm32_call(
                Api::ImmGetCompositionStringW,
                &[h, 0x80, 0, 0],
                call.thread_id,
                call.ansi,
                &mut ram
            )
            .unwrap(),
            1
        );
        assert_eq!(
            c.complete_imm32_call(
                Api::ImmGetCompositionStringW,
                &[0, 8, 0, 0],
                call.thread_id,
                call.ansi,
                &mut ram
            )
            .unwrap(),
            (-2i32) as u32
        );
    }
}
