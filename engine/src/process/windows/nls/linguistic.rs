use super::{NlsRuntime32, buffers};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        nls::{NlsApi32, NlsError32, linguistics},
        text::ordinal_key,
    },
};
impl NlsRuntime32 {
    pub(super) fn linguistic(
        &self,
        memory: &mut AddressSpace,
        api: NlsApi32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        let cp = self.profile.ansi;
        let wide = api.name().ends_with('W');
        let limit = self.profile.max_text_units;
        match api {
            LCMapStringA | LCMapStringW => {
                let locale = self.profile.locale(args[0])?;
                let cp = self.locale_code_page(locale, args[1])?;
                let source = buffers::units(memory, args[2], args[3] as i32, wide, cp, limit)?;
                let flags = args[1] & !0x40000000;
                if flags & 0x400 != 0 {
                    if flags & (0x300 | 0xf00000) != 0 {
                        return Err(NlsError32::InvalidFlags);
                    }
                    let input = source.strip_suffix(&[0]).unwrap_or(&source);
                    let mut key = linguistics::sort_key_for_locale(input, flags & !0xc00, locale)?;
                    if flags & 0x800 != 0 {
                        for pair in key.chunks_exact_mut(2) {
                            pair.swap(0, 1);
                        }
                    }
                    return buffers::output(memory, args[4], args[5] as i32, &key, 1);
                }
                if flags & 0x800 != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                if args[2] == args[4] && flags & !0x09000300 != 0 {
                    return Err(NlsError32::InvalidParameter);
                }
                let units = linguistics::map_string_for_locale(&source, flags, locale)?;
                let output = buffers::encoded(&units, wide, cp)?;
                buffers::output(
                    memory,
                    args[4],
                    args[5] as i32,
                    &output,
                    if wide { 2 } else { 1 },
                )
            }
            CompareStringA | CompareStringW => {
                let locale = self.profile.locale(args[0])?;
                let cp = self.locale_code_page(locale, args[1])?;
                let read = |address, count: u32| {
                    if count == 0 {
                        Ok(Vec::new())
                    } else {
                        buffers::units(memory, address, count as i32, wide, cp, limit)
                    }
                };
                let left = read(args[2], args[3])?;
                let right = read(args[4], args[5])?;
                let left = if (args[3] as i32) < 0 {
                    left.strip_suffix(&[0]).unwrap_or(&left)
                } else {
                    &left
                };
                let right = if (args[5] as i32) < 0 {
                    right.strip_suffix(&[0]).unwrap_or(&right)
                } else {
                    &right
                };
                let order =
                    linguistics::compare_for_locale(left, right, args[1] & !0x40000000, locale)?;
                Ok((order as i32 + 2) as u32)
            }
            CompareStringOrdinal => {
                if args[4] > 1 {
                    return Err(NlsError32::InvalidParameter);
                }
                let read = |address, count: u32| {
                    if count == 0 {
                        Ok(Vec::new())
                    } else {
                        buffers::units(memory, address, count as i32, true, cp, limit)
                    }
                };
                let mut left = read(args[0], args[1])?;
                let mut right = read(args[2], args[3])?;
                if (args[1] as i32) < 0 {
                    left.pop();
                }
                if (args[3] as i32) < 0 {
                    right.pop();
                }
                if args[4] != 0 {
                    left = ordinal_key(&left);
                    right = ordinal_key(&right);
                }
                Ok((left.cmp(&right) as i32 + 2) as u32)
            }
            GetStringTypeA | GetStringTypeW | GetStringTypeExA | GetStringTypeExW => {
                let no_locale = api == GetStringTypeW;
                let base = usize::from(!no_locale);
                if !no_locale {
                    self.profile.locale(args[0])?;
                }
                let cp = if no_locale {
                    cp
                } else {
                    self.locale_code_page(self.profile.locale(args[0])?, 0)?
                };
                let kind = args[base];
                let count = args[base + 2] as i32;
                let count = if count < 0 { -1 } else { count };
                let source = buffers::units(memory, args[base + 1], count, wide, cp, limit)?;
                if args[base + 1] == args[base + 3] {
                    return Err(NlsError32::InvalidParameter);
                }
                let values = linguistics::character_types(&source, kind)?;
                let bytes = buffers::encoded(&values, true, cp)?;
                if args[base + 3] == 0 {
                    return Err(NlsError32::InvalidParameter);
                }
                memory.write(GuestAddress(args[base + 3]), &bytes)?;
                Ok(1)
            }
            LstrlenA | LstrlenW => {
                if args[0] == 0 {
                    return Ok(0);
                }
                let bytes = buffers::bytes(memory, args[0], -1, if wide { 2 } else { 1 }, limit)?;
                Ok((bytes.len() / if wide { 2 } else { 1 } - 1) as u32)
            }
            LstrcmpA | LstrcmpW | LstrcmpiA | LstrcmpiW => {
                let left = buffers::units(memory, args[0], -1, wide, cp, limit)?;
                let right = buffers::units(memory, args[1], -1, wide, cp, limit)?;
                let flags = u32::from(matches!(api, LstrcmpiA | LstrcmpiW));
                Ok(linguistics::compare_for_locale(
                    &left[..left.len() - 1],
                    &right[..right.len() - 1],
                    flags,
                    self.profile.locale,
                )? as i32 as u32)
            }
            LstrcpyA | LstrcpyW | LstrcpynA | LstrcpynW | LstrcatA | LstrcatW => {
                let width = if wide { 2 } else { 1 };
                let bounded = matches!(api, LstrcpynA | LstrcpynW);
                if bounded && (args[2] as i32) <= 0 {
                    return Ok(args[0]);
                }
                if bounded && args[2] as usize > limit {
                    return Err(NlsError32::Capacity);
                }
                let source = if bounded {
                    buffers::prefix(memory, args[1], width, args[2] as usize - 1)?
                } else {
                    buffers::bytes(memory, args[1], -1, width, limit)?
                };
                let mut at = args[0];
                if matches!(api, LstrcatA | LstrcatW) {
                    let prior = buffers::bytes(memory, args[0], -1, width, limit)?;
                    at = at
                        .checked_add((prior.len() - width) as u32)
                        .ok_or(NlsError32::InvalidParameter)?;
                }
                if at == 0 {
                    return Err(NlsError32::InvalidParameter);
                }
                memory.write(GuestAddress(at), &source)?;
                Ok(args[0])
            }
            _ => unreachable!(),
        }
    }
}
