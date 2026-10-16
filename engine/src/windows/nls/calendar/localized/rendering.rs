use super::super::{DateLayout32, layout::DateLayoutWriter32};
use super::CalendarId32;
use crate::windows::nls::{
    NlsError32,
    calendar::{SystemTime32, from_filetime, to_filetime},
    locales::locale_identity,
};
use icu_calendar::{
    Date, Gregorian,
    cal::{
        Buddhist, Hebrew, Hijri, Japanese, Persian, Roc,
        hijri::{TabularAlgorithm, TabularAlgorithmEpoch, TabularAlgorithmLeapYears, UmmAlQura},
    },
};
use icu_datetime::{
    input::{DateTime, Time},
    pattern::{DateTimePattern, FixedCalendarDateTimeNames},
};
use writeable::TryWriteable;

pub fn format_calendar_date(
    time: SystemTime32,
    pattern: &str,
    locale: u32,
    calendar: u32,
) -> Result<String, NlsError32> {
    format_calendar_date_with_layout(time, pattern, locale, calendar, None)
}

pub fn format_calendar_date_with_layout(
    time: SystemTime32,
    pattern: &str,
    locale: u32,
    calendar: u32,
    layout: Option<DateLayout32>,
) -> Result<String, NlsError32> {
    let time = SystemTime32 {
        hour: 0,
        minute: 0,
        second: 0,
        millisecond: 0,
        ..time
    };
    let time = from_filetime(to_filetime(time)?)?;
    render_calendar_date(time, pattern, locale, calendar, layout)
}

pub(crate) fn format_civil_calendar_date(
    time: SystemTime32,
    pattern: &str,
    locale: u32,
    calendar: u32,
) -> Result<String, NlsError32> {
    render_calendar_date(
        super::super::civil::validated(time)?,
        pattern,
        locale,
        calendar,
        None,
    )
}

fn render_calendar_date(
    time: SystemTime32,
    pattern: &str,
    locale: u32,
    calendar: u32,
    layout: Option<DateLayout32>,
) -> Result<String, NlsError32> {
    let calendar = CalendarId32::from_id(calendar)?;
    let gregorian = Date::try_new_gregorian(i32::from(time.year), time.month as u8, time.day as u8)
        .map_err(|_| NlsError32::InvalidParameter)?;
    let date = gregorian.to_calendar(calendar.calendar());
    let year = date.year().era_year_or_related_iso()
        + if calendar == CalendarId32::Korean {
            2333
        } else {
            0
        };
    let month = date.month().ordinal;
    let day = date.day_of_month().0;
    if calendar == CalendarId32::Hebrew && year > 5999 {
        return Err(NlsError32::InvalidParameter);
    }
    let mut output = String::new();
    let mut layout = DateLayoutWriter32::new(pattern, layout)?;
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;
    while let Some(character) = chars.next() {
        if character == '\'' {
            if quoted && chars.peek() == Some(&'\'') {
                output.push('\'');
                chars.next();
            } else {
                if !quoted {
                    layout.mark(output.len());
                }
                quoted = !quoted;
            }
            continue;
        }
        if quoted {
            output.push(character);
            continue;
        }
        let mut length = 1;
        while chars.peek() == Some(&character) {
            chars.next();
            length += 1;
        }
        if matches!(character, 'd' | 'M' | 'y' | 'g') {
            layout.mark(output.len());
        }
        if calendar == CalendarId32::Hebrew {
            let numeral = match (character, length) {
                ('d', 1 | 2) => Some(u16::from(day)),
                ('M', 1 | 2) => Some(u16::from(month)),
                ('y', _) => Some((year % 1000) as u16),
                _ => None,
            };
            if let Some(numeral) = numeral {
                super::hebrew::append_number(&mut output, numeral)?;
                continue;
            }
        }
        match (character, length) {
            ('d', 1) => output.push_str(&day.to_string()),
            ('d', 2) => output.push_str(&format!("{day:02}")),
            ('d', _) => output.push_str(&symbol(
                time,
                locale,
                calendar,
                if length == 3 { "EEE" } else { "EEEE" },
            )?),
            ('M', 1) => output.push_str(&month.to_string()),
            ('M', 2) => output.push_str(&format!("{month:02}")),
            ('M', _) => output.push_str(&symbol(
                time,
                locale,
                calendar,
                if length == 3 { "MMM" } else { "MMMM" },
            )?),
            ('y', 1) => output.push_str(&year.rem_euclid(100).to_string()),
            ('y', 2) => output.push_str(&format!("{:02}", year.rem_euclid(100))),
            ('y', _) => output.push_str(&format!("{year:04}")),
            ('g', _) => output.push_str(&symbol(time, locale, calendar, "G")?),
            _ => output.push_str(&character.to_string().repeat(length)),
        }
    }
    layout.finish(output)
}

pub fn calendar_text(
    locale: u32,
    calendar: u32,
    kind: u32,
) -> Result<(String, Option<u32>), NlsError32> {
    let genitive = kind & 0x10000000 != 0;
    let kind = kind & 0xffff;
    let calendar = CalendarId32::from_id(calendar)?;
    let locale = calendar.naming_locale(locale);
    let sample = SystemTime32 {
        year: 2024,
        month: 7,
        day: 1,
        ..Default::default()
    };
    let gregorian = Date::try_new_gregorian(2024, 7, 1).unwrap();
    let cal = calendar.calendar();
    let date = gregorian.to_calendar(icu_calendar::Ref(&cal));
    let value = match kind {
        1 => return number(calendar.id()),
        2 => match calendar {
            CalendarId32::Gregorian | CalendarId32::GregorianUs => "Gregorian Calendar",
            CalendarId32::Japanese => "Japanese Calendar",
            CalendarId32::Taiwan => "Taiwan Calendar",
            CalendarId32::Korean => "Korean Tangun Calendar",
            CalendarId32::Hijri => "Hijri Calendar",
            CalendarId32::Thai => "Thai Buddhist Calendar",
            CalendarId32::Hebrew => "Hebrew Calendar",
            CalendarId32::GregorianFrench => "Gregorian Middle East French Calendar",
            CalendarId32::GregorianArabic => "Gregorian Arabic Calendar",
            CalendarId32::GregorianTransliteratedEnglish => {
                "Gregorian Transliterated English Calendar"
            }
            CalendarId32::GregorianTransliteratedFrench => {
                "Gregorian Transliterated French Calendar"
            }
            CalendarId32::Persian => "Persian Calendar",
            CalendarId32::UmAlQura => "Um Al Qura Calendar",
        }
        .into(),
        3 => {
            return number(match calendar {
                CalendarId32::Japanese => 2019,
                CalendarId32::Taiwan => 1912,
                CalendarId32::Korean => 2333,
                CalendarId32::Thai => 543,
                _ => 0,
            });
        }
        4 => symbol(sample, locale, calendar, "G")?,
        7..=20 => {
            let day = (kind - if kind <= 13 { 7 } else { 14 }) as u16 + 1;
            symbol(
                SystemTime32 {
                    year: 2024,
                    month: 1,
                    day,
                    ..Default::default()
                },
                locale,
                calendar,
                if kind <= 13 { "EEEE" } else { "EEE" },
            )?
        }
        21..=46 => {
            let short = kind >= 34;
            let month = (kind - if short { 34 } else { 21 }) as u8 + 1;
            if month > date.months_in_year() {
                return Ok((String::new(), None));
            }
            let code = if calendar == CalendarId32::Hebrew && date.months_in_year() == 13 {
                if month == 6 {
                    icu_calendar::types::MonthCode::new_leap(5)
                } else {
                    icu_calendar::types::MonthCode::new_normal(month - u8::from(month > 6))
                }
            } else {
                icu_calendar::types::MonthCode::new_normal(month)
            }
            .ok_or(NlsError32::InvalidParameter)?;
            let date = Date::try_new_from_codes(None, date.extended_year(), code, 1, cal.clone())
                .map_err(|_| NlsError32::InvalidParameter)?
                .to_calendar(Gregorian);
            let time = SystemTime32 {
                year: date.year().era_year_or_related_iso() as u16,
                month: u16::from(date.month().ordinal),
                day: u16::from(date.day_of_month().0),
                ..Default::default()
            };
            symbol(
                time,
                locale,
                calendar,
                match (short, genitive) {
                    (true, true) => "MMM",
                    (false, true) => "MMMM",
                    (true, false) => "LLL",
                    (false, false) => "LLLL",
                },
            )?
        }
        0x31..=0x37 => symbol(
            SystemTime32 {
                year: 2024,
                month: 1,
                day: (kind - 0x31 + 1) as u16,
                ..Default::default()
            },
            locale,
            calendar,
            "EEEEEE",
        )?,
        0x30 => {
            return number(match calendar {
                CalendarId32::Japanese | CalendarId32::Taiwan => 99,
                CalendarId32::Thai => 2572,
                CalendarId32::Korean => 4362,
                CalendarId32::Hebrew => 5790,
                CalendarId32::Hijri | CalendarId32::UmAlQura => 1451,
                CalendarId32::Persian => 1408,
                _ => 2029,
            });
        }
        _ => return Err(NlsError32::InvalidFlags),
    };
    Ok((value, None))
}
fn number(value: u32) -> Result<(String, Option<u32>), NlsError32> {
    Ok((value.to_string(), Some(value)))
}
fn symbol(
    time: SystemTime32,
    locale: u32,
    calendar: CalendarId32,
    pattern: &str,
) -> Result<String, NlsError32> {
    if calendar == CalendarId32::Hebrew && pattern == "G" {
        return Ok("C.E.".to_owned());
    }
    let locale = calendar.naming_locale(locale);
    let locale: icu_locale::Locale = locale_identity(locale)?
        .language_tag()
        .parse()
        .map_err(|_| NlsError32::InvalidParameter)?;
    let pattern: DateTimePattern = pattern.parse().map_err(|_| NlsError32::InvalidParameter)?;
    let date = Date::try_new_gregorian(i32::from(time.year), time.month as u8, time.day as u8)
        .map_err(|_| NlsError32::InvalidParameter)?;
    macro_rules! render {
        ($kind:ty,$calendar:expr) => {{
            let mut names = FixedCalendarDateTimeNames::<$kind>::try_new(locale.into())
                .map_err(|_| NlsError32::Unsupported)?;
            let formatter = names
                .include_for_pattern(&pattern)
                .map_err(|_| NlsError32::Unsupported)?;
            formatter
                .format(&DateTime {
                    date: date.to_calendar($calendar),
                    time: Time::start_of_day(),
                })
                .try_write_to_string()
                .map(|value| value.into_owned())
                .map_err(|_| NlsError32::Unsupported)
        }};
    }
    match calendar {
        CalendarId32::Gregorian
        | CalendarId32::GregorianUs
        | CalendarId32::Korean
        | CalendarId32::GregorianFrench
        | CalendarId32::GregorianArabic
        | CalendarId32::GregorianTransliteratedEnglish
        | CalendarId32::GregorianTransliteratedFrench => {
            render!(Gregorian, Gregorian)
        }
        CalendarId32::Japanese => render!(Japanese, Japanese::new()),
        CalendarId32::Taiwan => render!(Roc, Roc),
        CalendarId32::Thai => render!(Buddhist, Buddhist),
        CalendarId32::Hebrew => render!(Hebrew, Hebrew),
        CalendarId32::Persian => render!(Persian, Persian),
        CalendarId32::Hijri => render!(
            Hijri<TabularAlgorithm>,
            Hijri::new_tabular(
                TabularAlgorithmLeapYears::TypeII,
                TabularAlgorithmEpoch::Friday
            )
        ),
        CalendarId32::UmAlQura => render!(Hijri<UmmAlQura>, Hijri::new_umm_al_qura()),
    }
}

pub fn calendar_patterns(
    locale: u32,
    calendar: u32,
) -> Result<(String, String, String), NlsError32> {
    use icu_datetime::{
        DateTimeFormatter,
        fieldsets::{YM, YMD, YMDE},
    };
    let calendar = CalendarId32::from_id(calendar)?;
    let locale = calendar.naming_locale(locale);
    let name = locale_identity(locale)?.name;
    let calendar_name = match calendar {
        CalendarId32::Japanese => "japanese",
        CalendarId32::Taiwan => "roc",
        CalendarId32::Hijri => "islamic-civil",
        CalendarId32::Thai => "buddhist",
        CalendarId32::Hebrew => "hebrew",
        CalendarId32::Persian => "persian",
        CalendarId32::UmAlQura => "islamic-umalqura",
        _ => "gregory",
    };
    let locale: icu_locale::Locale = format!("{name}-u-ca-{calendar_name}")
        .parse()
        .map_err(|_| NlsError32::InvalidParameter)?;
    let date = Date::try_new_gregorian(2024, 7, 1).unwrap();
    let short = DateTimeFormatter::try_new(locale.clone().into(), YMD::short())
        .map_err(|_| NlsError32::Unsupported)?
        .format(&date)
        .pattern()
        .to_string();
    let long = DateTimeFormatter::try_new(locale.clone().into(), YMDE::long())
        .map_err(|_| NlsError32::Unsupported)?
        .format(&date)
        .pattern()
        .to_string();
    let year_month = DateTimeFormatter::try_new(locale.into(), YM::long())
        .map_err(|_| NlsError32::Unsupported)?
        .format(&date)
        .pattern()
        .to_string();
    Ok((
        super::super::super::locales::data::windows_pattern(&short)?,
        super::super::super::locales::data::windows_pattern(&long)?,
        super::super::super::locales::data::windows_pattern(&year_month)?,
    ))
}

pub fn calendar_month_day_pattern(locale: u32, calendar: u32) -> Result<String, NlsError32> {
    use icu_datetime::{DateTimeFormatter, fieldsets::MD};
    let calendar = CalendarId32::from_id(calendar)?;
    let locale = calendar.naming_locale(locale);
    let name = locale_identity(locale)?.name;
    let suffix = match calendar {
        CalendarId32::Japanese => "japanese",
        CalendarId32::Taiwan => "roc",
        CalendarId32::Hijri => "islamic-civil",
        CalendarId32::Thai => "buddhist",
        CalendarId32::Hebrew => "hebrew",
        CalendarId32::Persian => "persian",
        CalendarId32::UmAlQura => "islamic-umalqura",
        _ => "gregory",
    };
    let locale: icu_locale::Locale = format!("{name}-u-ca-{suffix}")
        .parse()
        .map_err(|_| NlsError32::InvalidParameter)?;
    let date = Date::try_new_gregorian(2024, 7, 1).unwrap();
    let pattern = DateTimeFormatter::try_new(locale.into(), MD::long())
        .map_err(|_| NlsError32::Unsupported)?
        .format(&date)
        .pattern()
        .to_string();
    super::super::super::locales::data::windows_pattern(&pattern)
}
