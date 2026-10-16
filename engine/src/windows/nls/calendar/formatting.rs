use super::{DateLayout32, SystemTime32, from_filetime, layout::DateLayoutWriter32, to_filetime};
use crate::windows::nls::NlsError32;
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
pub fn format_date(time: SystemTime32, pattern: &str) -> Result<String, NlsError32> {
    let time = SystemTime32 {
        hour: 0,
        minute: 0,
        second: 0,
        millisecond: 0,
        ..time
    };
    let time = from_filetime(to_filetime(time)?)?;
    format(time, pattern, true, None, None)
}
pub fn format_time(time: SystemTime32, pattern: &str) -> Result<String, NlsError32> {
    if time.hour > 23 || time.minute > 59 || time.second > 59 {
        return Err(NlsError32::InvalidParameter);
    }
    format(time, pattern, false, None, None)
}
pub fn format_date_with_names(
    time: SystemTime32,
    pattern: &str,
    names: &crate::windows::nls::locales::CalendarNames32,
) -> Result<String, NlsError32> {
    format_date_with_layout(time, pattern, names, None)
}
pub fn format_date_with_layout(
    time: SystemTime32,
    pattern: &str,
    names: &crate::windows::nls::locales::CalendarNames32,
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
    format(time, pattern, true, Some(names), layout)
}
pub fn format_time_with_names(
    time: SystemTime32,
    pattern: &str,
    names: &crate::windows::nls::locales::CalendarNames32,
) -> Result<String, NlsError32> {
    if time.hour > 23 || time.minute > 59 || time.second > 59 {
        return Err(NlsError32::InvalidParameter);
    }
    format(time, pattern, false, Some(names), None)
}
pub(crate) fn format_civil_date_with_names(
    time: SystemTime32,
    pattern: &str,
    names: &crate::windows::nls::locales::CalendarNames32,
) -> Result<String, NlsError32> {
    format(
        super::civil::validated(time)?,
        pattern,
        true,
        Some(names),
        None,
    )
}
fn format(
    time: SystemTime32,
    pattern: &str,
    date: bool,
    names: Option<&crate::windows::nls::locales::CalendarNames32>,
    layout: Option<DateLayout32>,
) -> Result<String, NlsError32> {
    let genitive = date && has_numeric_day(pattern);
    let mut layout = DateLayoutWriter32::new(pattern, layout)?;
    let mut out = String::new();
    out.try_reserve(pattern.len().saturating_mul(8))
        .map_err(|_| NlsError32::Allocation)?;
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if c == '\'' {
            if quoted && chars.peek() == Some(&'\'') {
                out.push('\'');
                chars.next();
            } else {
                if !quoted {
                    layout.mark(out.len());
                }
                quoted = !quoted;
            }
            continue;
        }
        if quoted {
            out.push(c);
            continue;
        }
        let mut n = 1;
        while chars.peek() == Some(&c) {
            chars.next();
            n += 1;
        }
        if date && matches!(c, 'd' | 'M' | 'y' | 'g') {
            layout.mark(out.len());
        }
        let value = match (date, c, n) {
            (true, 'd', 1) => time.day.to_string(),
            (true, 'd', 2) => format!("{:02}", time.day),
            (true, 'd', 3) => names.map_or_else(
                || DAYS[time.day_of_week as usize][..3].into(),
                |names| names.abbreviated_days[(time.day_of_week as usize + 6) % 7].clone(),
            ),
            (true, 'd', _) => names.map_or_else(
                || DAYS[time.day_of_week as usize].into(),
                |names| names.days[(time.day_of_week as usize + 6) % 7].clone(),
            ),
            (true, 'M', 1) => time.month.to_string(),
            (true, 'M', 2) => format!("{:02}", time.month),
            (true, 'M', 3) => names.map_or_else(
                || MONTHS[time.month as usize - 1][..3].into(),
                |names| {
                    if genitive {
                        &names.genitive_abbreviated_months[time.month as usize - 1]
                    } else {
                        &names.abbreviated_months[time.month as usize - 1]
                    }
                    .clone()
                },
            ),
            (true, 'M', _) => names.map_or_else(
                || MONTHS[time.month as usize - 1].into(),
                |names| {
                    if genitive {
                        &names.genitive_months[time.month as usize - 1]
                    } else {
                        &names.months[time.month as usize - 1]
                    }
                    .clone()
                },
            ),
            (true, 'y', 1) => format!("{}", time.year % 100),
            (true, 'y', 2) => format!("{:02}", time.year % 100),
            (true, 'y', _) => format!("{:04}", time.year),
            (true, 'g', _) => names.map_or_else(|| "A.D.".into(), |names| names.era.clone()),
            (false, 'H', 1) => time.hour.to_string(),
            (false, 'H', _) => format!("{:02}", time.hour),
            (false, 'h', 1) => (if time.hour.is_multiple_of(12) {
                12
            } else {
                time.hour % 12
            })
            .to_string(),
            (false, 'h', _) => format!(
                "{:02}",
                if time.hour.is_multiple_of(12) {
                    12
                } else {
                    time.hour % 12
                }
            ),
            (false, 'm', 1) => time.minute.to_string(),
            (false, 'm', _) => format!("{:02}", time.minute),
            (false, 's', 1) => time.second.to_string(),
            (false, 's', _) => format!("{:02}", time.second),
            (false, 't', 1) => names.map_or_else(
                || if time.hour < 12 { "A" } else { "P" }.into(),
                |names| {
                    if time.hour < 12 { &names.am } else { &names.pm }
                        .chars()
                        .take(1)
                        .collect()
                },
            ),
            (false, 't', _) => names.map_or_else(
                || if time.hour < 12 { "AM" } else { "PM" }.into(),
                |names| {
                    if time.hour < 12 {
                        names.am.clone()
                    } else {
                        names.pm.clone()
                    }
                },
            ),
            _ => c.to_string().repeat(n),
        };
        out.push_str(&value);
    }
    layout.finish(out)
}

fn has_numeric_day(pattern: &str) -> bool {
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
            } else {
                quoted = !quoted;
            }
            continue;
        }
        if c == 'd' && !quoted {
            let mut count = 1;
            while chars.peek() == Some(&'d') {
                chars.next();
                count += 1;
            }
            if count <= 2 {
                return true;
            }
        }
    }
    false
}
