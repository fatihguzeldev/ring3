use crate::windows::nls::{NlsError32, calendar::SystemTime32};
use icu_calendar::Gregorian;
use icu_datetime::{
    FixedCalendarDateTimeFormatter,
    fieldsets::{MD, T, YM, YMD, YMDE},
    input::{Date, DateTime, Time},
    pattern::{DateTimePattern, FixedCalendarDateTimeNames},
};
use writeable::TryWriteable;

#[derive(Debug)]
pub struct CalendarNames32 {
    pub months: [String; 12],
    pub abbreviated_months: [String; 12],
    pub genitive_months: [String; 12],
    pub genitive_abbreviated_months: [String; 12],
    pub days: [String; 7],
    pub abbreviated_days: [String; 7],
    pub am: String,
    pub pm: String,
    pub era: String,
    pub short_date: String,
    pub long_date: String,
    pub year_month: String,
    pub month_day: String,
    pub time: String,
}
impl CalendarNames32 {
    pub(super) fn new(locale: &icu_locale::Locale) -> Result<Self, NlsError32> {
        let mut names = Self {
            months: std::array::from_fn(|_| String::new()),
            abbreviated_months: std::array::from_fn(|_| String::new()),
            genitive_months: std::array::from_fn(|_| String::new()),
            genitive_abbreviated_months: std::array::from_fn(|_| String::new()),
            days: std::array::from_fn(|_| String::new()),
            abbreviated_days: std::array::from_fn(|_| String::new()),
            am: String::new(),
            pm: String::new(),
            era: String::new(),
            short_date: String::new(),
            long_date: String::new(),
            year_month: String::new(),
            month_day: String::new(),
            time: String::new(),
        };
        for index in 0..12 {
            let time = SystemTime32 {
                year: 2024,
                month: index as u16 + 1,
                day: 1,
                ..Default::default()
            };
            names.months[index] = symbol(locale, "LLLL", time)?;
            names.abbreviated_months[index] = symbol(locale, "LLL", time)?;
            names.genitive_months[index] = symbol(locale, "MMMM", time)?;
            names.genitive_abbreviated_months[index] = symbol(locale, "MMM", time)?;
        }
        for index in 0..7 {
            let time = SystemTime32 {
                year: 2024,
                month: 1,
                day: index as u16 + 1,
                ..Default::default()
            };
            names.days[index] = symbol(locale, "EEEE", time)?;
            names.abbreviated_days[index] = symbol(locale, "EEE", time)?;
        }
        let sample = SystemTime32 {
            year: 2024,
            month: 1,
            day: 1,
            ..Default::default()
        };
        names.am = symbol(locale, "a", sample)?;
        names.pm = symbol(locale, "a", SystemTime32 { hour: 13, ..sample })?;
        names.era = symbol(locale, "G", sample)?;
        let datetime = datetime(sample)?;
        names.short_date = windows_pattern(
            &FixedCalendarDateTimeFormatter::<Gregorian, _>::try_new(
                locale.clone().into(),
                YMD::short(),
            )
            .map_err(|_| NlsError32::Unsupported)?
            .format(&datetime)
            .pattern()
            .to_string(),
        )?;
        names.long_date = windows_pattern(
            &FixedCalendarDateTimeFormatter::<Gregorian, _>::try_new(
                locale.clone().into(),
                YMDE::long(),
            )
            .map_err(|_| NlsError32::Unsupported)?
            .format(&datetime)
            .pattern()
            .to_string(),
        )?;
        names.year_month = windows_pattern(
            &FixedCalendarDateTimeFormatter::<Gregorian, _>::try_new(
                locale.clone().into(),
                YM::long(),
            )
            .map_err(|_| NlsError32::Unsupported)?
            .format(&datetime)
            .pattern()
            .to_string(),
        )?;
        names.month_day = windows_pattern(
            &FixedCalendarDateTimeFormatter::<Gregorian, _>::try_new(
                locale.clone().into(),
                MD::long(),
            )
            .map_err(|_| NlsError32::Unsupported)?
            .format(&datetime)
            .pattern()
            .to_string(),
        )?;
        names.time = windows_pattern(
            &FixedCalendarDateTimeFormatter::<Gregorian, _>::try_new(
                locale.clone().into(),
                T::medium(),
            )
            .map_err(|_| NlsError32::Unsupported)?
            .format(&datetime)
            .pattern()
            .to_string(),
        )?;
        if locale.id.to_string() == "en-US" {
            names.short_date = "M/d/yyyy".into();
            names.long_date = "dddd, MMMM d, yyyy".into();
            names.year_month = "MMMM yyyy".into();
            names.time = "h:mm:ss tt".into();
            names.era = "A.D.".into();
        }
        Ok(names)
    }
}
fn datetime(time: SystemTime32) -> Result<DateTime<Gregorian>, NlsError32> {
    Ok(DateTime {
        date: Date::try_new_gregorian(i32::from(time.year), time.month as u8, time.day as u8)
            .map_err(|_| NlsError32::InvalidParameter)?,
        time: Time::try_new(
            time.hour as u8,
            time.minute as u8,
            time.second as u8,
            u32::from(time.millisecond) * 1_000_000,
        )
        .map_err(|_| NlsError32::InvalidParameter)?,
    })
}
fn symbol(
    locale: &icu_locale::Locale,
    pattern: &str,
    time: SystemTime32,
) -> Result<String, NlsError32> {
    let pattern: DateTimePattern = pattern.parse().map_err(|_| NlsError32::InvalidParameter)?;
    let mut names = FixedCalendarDateTimeNames::<Gregorian>::try_new(locale.clone().into())
        .map_err(|_| NlsError32::Unsupported)?;
    let formatter = names
        .include_for_pattern(&pattern)
        .map_err(|_| NlsError32::Unsupported)?;
    formatter
        .format(&datetime(time)?)
        .try_write_to_string()
        .map(|value| value.into_owned())
        .map_err(|_| NlsError32::Unsupported)
}

pub(in crate::windows::nls) fn windows_pattern(pattern: &str) -> Result<String, NlsError32> {
    let mut output = String::new();
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;
    while let Some(character) = chars.next() {
        if character == '\'' {
            output.push(character);
            if chars.peek() == Some(&'\'') {
                output.push(chars.next().unwrap());
            } else {
                quoted = !quoted;
            }
            continue;
        }
        if quoted || !character.is_ascii_alphabetic() {
            output.push(character);
            continue;
        }
        let mut length = 1;
        while chars.peek() == Some(&character) {
            chars.next();
            length += 1;
        }
        match character {
            'E' | 'e' | 'c' => output.push_str(if length >= 4 { "dddd" } else { "ddd" }),
            'L' | 'M' => output.push_str(&"M".repeat(length.min(4))),
            'y' | 'u' | 'r' => output.push_str(&"y".repeat(if length == 2 { 2 } else { 4 })),
            'G' => output.push_str("gg"),
            'a' | 'b' | 'B' => output.push_str("tt"),
            'K' => output.push_str(&"h".repeat(length.min(2))),
            'k' => output.push_str(&"H".repeat(length.min(2))),
            'd' | 'H' | 'h' | 'm' | 's' => output.push_str(&character.to_string().repeat(length)),
            _ => return Err(NlsError32::Unsupported),
        }
    }
    if quoted {
        return Err(NlsError32::InvalidParameter);
    }
    Ok(output)
}
