use super::super::{NlsRuntime32, buffers};
use crate::{
    memory::AddressSpace,
    windows::nls::{
        NlsApi32, NlsError32,
        locales::{locale_from_name, locale_to_name},
    },
};
impl NlsRuntime32 {
    pub(in crate::process::windows::nls) fn named_locale(
        &self,
        memory: &AddressSpace,
        address: u32,
        allow_neutral: bool,
    ) -> Result<u32, NlsError32> {
        if address == 0 {
            return Ok(self.profile.locale);
        }
        let units = buffers::units(memory, address, -1, true, self.profile.ansi, 85)?;
        let name = String::from_utf16(&units[..units.len() - 1])
            .map_err(|_| NlsError32::InvalidParameter)?;
        if name == "!x-sys-default-locale" {
            return Ok(self.profile.locale);
        }
        locale_from_name(&name, allow_neutral)
    }
    pub(in crate::process::windows::nls) fn names(
        &mut self,
        memory: &mut AddressSpace,
        thread: u32,
        api: NlsApi32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        let (locale, address, capacity) = match api {
            GetUserDefaultLocaleName => {
                if args[0] == 0 || args[1] == 0 {
                    return Err(NlsError32::InsufficientBuffer);
                }
                (self.profile.locale, args[0], args[1])
            }
            LCIDToLocaleName => {
                if args[3] & !0x08000000 != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                let locale = self.profile.locale(args[0])?;
                let name = locale_to_name(locale, args[3] & 0x08000000 != 0)?;
                return output(memory, args[1], args[2], name);
            }
            LocaleNameToLCID => {
                if args[1] & !0x08000000 != 0 {
                    return Err(NlsError32::InvalidFlags);
                }
                return self.named_locale(memory, args[0], args[1] & 0x08000000 != 0);
            }
            IsValidLocaleName => {
                return Ok(u32::from(self.named_locale(memory, args[0], true).is_ok()));
            }
            GetLocaleInfoEx => {
                let locale = self.named_locale(memory, args[0], true)?;
                return self.locale_call(
                    memory,
                    thread,
                    GetLocaleInfoW,
                    &[locale, args[1], args[2], args[3]],
                );
            }
            _ => unreachable!(),
        };
        output(memory, address, capacity, locale_to_name(locale, true)?)
    }
}
fn output(
    memory: &mut AddressSpace,
    address: u32,
    capacity: u32,
    name: &str,
) -> Result<u32, NlsError32> {
    let bytes: Vec<u8> = name
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    buffers::output(memory, address, capacity as i32, &bytes, 2)
}
