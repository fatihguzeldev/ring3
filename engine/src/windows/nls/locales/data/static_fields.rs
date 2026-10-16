use super::regions::region_data;
use crate::windows::nls::{
    NlsError32,
    locales::{LocaleIdentity32, locale_identity},
};

pub fn locale_number(locale: u32, kind: u32) -> Result<Option<u32>, NlsError32> {
    let identity = locale_identity(locale)?;
    let region_name = identity
        .name
        .rsplit('-')
        .next()
        .filter(|part| part.len() == 2 && part.as_bytes().iter().all(u8::is_ascii_uppercase))
        .unwrap_or("");
    let region = region_data(region_name);
    Ok(Some(match kind & 0xffff {
        1 | 9 => locale & 0xffff,
        5 | 0xa => region.dialing,
        0xb => oem(identity.ansi, region_name),
        0xd => u32::from(!region.metric),
        0x11 => 2,
        0x12 => 1,
        0x19 | 0x1a => u32::from(region.currency_digits),
        0x1004 => identity.ansi,
        0x1009 => u32::from(region.calendar),
        0x100a => u32::from(region.paper),
        0x100b => alternate_calendar(identity, region_name, region.calendar),
        0x100c => u32::from(region.first_day),
        0x100d => u32::from(region.first_week),
        0x1010 => 1,
        0x1011 => match identity.ansi {
            1251 => 10007,
            1253 => 10006,
            1254 => 10081,
            1255 => 10005,
            1256 => 10004,
            932 => 10001,
            936 => 10008,
            949 => 10003,
            950 => 10002,
            _ => 10000,
        },
        0x1012 => 37,
        0x1014 => 1,
        0x71 => u32::from(region_name.is_empty()),
        0x7d => 0,
        _ => return Ok(None),
    }))
}
pub(super) fn alternate_calendar(identity: LocaleIdentity32, region: &str, calendar: u8) -> u32 {
    match identity.lcid {
        0x409 | 0x420 => return 2,
        0x401 | 0x465 => return 6,
        0xc01 => return 23,
        _ => {}
    }
    if calendar != 1 {
        return 1;
    }
    match identity.language {
        "ja" => 3,
        "ko" => 5,
        "he" => 8,
        "zh" if region == "TW" => 4,
        _ => 0,
    }
}

pub fn static_information(
    locale: u32,
    kind: u32,
) -> Result<Option<(String, Option<u32>)>, NlsError32> {
    if let Some(value) = locale_number(locale, kind)? {
        return Ok(Some((
            if matches!(kind, 1 | 9) {
                format!("{value:04x}")
            } else {
                value.to_string()
            },
            Some(value),
        )));
    }
    let identity = locale_identity(locale)?;
    let value = match kind {
        3 => identity.windows_three_letter,
        0x59 => identity.language,
        0x5c => identity.windows_name(),
        0x67 => identity.language_three_letter,
        0x100e | 0x100f => "",
        0x1013 => identity.sort_name(),
        _ => return Ok(None),
    };
    Ok(Some((value.into(), None)))
}
pub(super) fn oem(ansi: u32, region: &str) -> u32 {
    match ansi {
        1250 => 852,
        1251 => 866,
        1252 => {
            if matches!(region, "US" | "CA") {
                437
            } else {
                850
            }
        }
        1253 => 737,
        1254 => 857,
        1255 => 862,
        1256 => 720,
        1257 => 775,
        1258 => 1258,
        874 | 932 | 936 | 949 | 950 => ansi,
        _ => 0,
    }
}
