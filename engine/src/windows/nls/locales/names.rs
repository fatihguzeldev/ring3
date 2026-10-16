use super::{LocaleIdentity32, locale_identity};
use crate::windows::nls::NlsError32;

pub fn installed_locales() -> impl Iterator<Item = LocaleIdentity32> {
    super::inventory::LOCALE_IDS
        .iter()
        .copied()
        .filter_map(|id| locale_identity(id).ok())
        .chain(
            [
                0x10407, 0x1040e, 0x10437, 0x20804, 0x30404, 0x40404, 0x40c04, 0x41404, 0x21004,
                0x40411,
            ]
            .into_iter()
            .filter_map(|id| locale_identity(id).ok()),
        )
}
pub fn locale_is_neutral(locale: LocaleIdentity32) -> bool {
    locale.lcid != 0x7f
        && !locale.name.split('-').skip(1).any(|part| {
            part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_uppercase())
                || part.len() == 3 && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}
pub fn locale_from_name(name: &str, allow_neutral: bool) -> Result<u32, NlsError32> {
    let locale = installed_locales()
        .find(|locale| locale.windows_name().eq_ignore_ascii_case(name))
        .ok_or(NlsError32::InvalidParameter)?;
    if allow_neutral || !locale_is_neutral(locale) {
        return Ok(locale.lcid);
    }
    specific_locale(locale.lcid)
}
pub fn locale_to_name(locale: u32, allow_neutral: bool) -> Result<&'static str, NlsError32> {
    let identity = locale_identity(locale)?;
    let locale = if !allow_neutral && locale_is_neutral(identity) {
        locale_identity(specific_locale(locale)?)?
    } else {
        identity
    };
    Ok(locale.windows_name())
}
fn specific_locale(locale: u32) -> Result<u32, NlsError32> {
    let specific = match locale {
        4 => 0x804,
        0x7c04 => 0x404,
        _ => (locale & 0x3ff) | 0x400,
    };
    locale_identity(specific).map(|locale| locale.lcid)
}
