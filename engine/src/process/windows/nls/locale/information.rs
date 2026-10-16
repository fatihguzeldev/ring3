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
                let locale = self.profile.locale(args[0])?;
                let calendar = matches!(api, GetCalendarInfoA | GetCalendarInfoW);
                let wide = api.name().ends_with('W');
                let base = usize::from(calendar);
                let kind = args[1 + base];
                if kind & 0x0fff0000 != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                if kind & 0x10000000 != 0
                    && !(if calendar {
                        matches!(kind & 0xffff, 21..=46)
                    } else {
                        matches!(kind & 0xffff, 0x38..=0x4f | 0x100e | 0x100f)
                    })
                {
                    return Err(NlsError32::InvalidFlags);
                }
                if !calendar
                    && kind & 0x20000000 != 0
                    && let Some(value) = crate::windows::nls::locales::locale_number(locale, kind)?
                {
                    return buffers::output(
                        memory,
                        args[2],
                        args[3] as i32,
                        &value.to_le_bytes(),
                        if wide { 2 } else { 1 },
                    );
                }
                let (text, numeric) = if calendar {
                    self.data(locale)?
                        .calendar_information(args[1], kind & 0x1000ffff)?
                } else if let Some(value) =
                    crate::windows::nls::locales::static_information(locale, kind & 0x1000ffff)?
                {
                    value
                } else {
                    self.data(locale)?.information(kind & 0x1000ffff)?
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
                let bytes = buffers::encoded(&units, wide, self.locale_code_page(locale, kind)?)?;
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
