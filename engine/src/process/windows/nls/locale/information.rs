use super::super::{NlsRuntime32, buffers};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::nls::{NlsApi32, NlsError32},
};
impl NlsRuntime32 {
    pub(in crate::process::windows::nls) fn locale_call(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        api: NlsApi32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        match api {
            GetUserDefaultLCID | GetSystemDefaultLCID => Ok(self.profile.locale),
            GetUserDefaultLangID | GetSystemDefaultLangID => Ok(self.profile.locale & 0xffff),
            GetThreadLocale => Ok(self.locale(thread)),
            GetThreadUILanguage => Ok(u32::from(self.language(thread))),
            SetThreadLocale => {
                let locale = self.profile.locale(args[0])?;
                self.set(thread, Some(locale), None)?;
                Ok(1)
            }
            SetThreadUILanguage => {
                let language = if args[0] == 0 {
                    self.profile.locale
                } else {
                    self.profile.locale(args[0])?
                };
                self.set(thread, None, Some(language as u16))?;
                Ok(language & 0xffff)
            }
            IsValidLocale => {
                if args[1] & !3 != 0 || args[1] == 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                Ok(u32::from(self.profile.locale(args[0]).is_ok()))
            }
            GetLocaleInfoA | GetLocaleInfoW | GetCalendarInfoA | GetCalendarInfoW => {
                self.profile.locale(args[0])?;
                let calendar = matches!(api, GetCalendarInfoA | GetCalendarInfoW);
                let wide = api.name().ends_with('W');
                let base = usize::from(calendar);
                if calendar && args[1] != 1 {
                    return Err(NlsError32::InvalidParameter);
                }
                let kind = args[1 + base];
                if kind & 0x1fff0000 != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                let (text, numeric) = if calendar {
                    super::data::calendar(kind & 0xffff)?
                } else {
                    super::data::value(kind & 0xffff)?
                };
                if kind & 0x20000000 != 0 {
                    let value = numeric.ok_or(NlsError32::InvalidFlags)?;
                    if calendar {
                        if args[3] != 0 || args[4] != 0 || args[5] == 0 {
                            return Err(NlsError32::InvalidParameter);
                        }
                        memory.write(GuestAddress(args[5]), &value.to_le_bytes())?;
                        return Ok(if wide { 2 } else { 4 });
                    }
                    return buffers::output(
                        memory,
                        args[2],
                        args[3] as i32,
                        &value.to_le_bytes(),
                        if wide { 2 } else { 1 },
                    );
                }
                if calendar && args[5] != 0 {
                    return Err(NlsError32::InvalidParameter);
                }
                let units = text.encode_utf16().chain([0]).collect::<Vec<_>>();
                let bytes = buffers::encoded(&units, wide, self.profile.ansi)?;
                buffers::output(
                    memory,
                    args[2 + base],
                    args[3 + base] as i32,
                    &bytes,
                    if wide { 2 } else { 1 },
                )
            }
            _ => unreachable!(),
        }
    }
}
