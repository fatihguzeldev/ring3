use super::{CalendarId32, SystemTime32};
use crate::windows::nls::NlsError32;
use icu_calendar::{Date, Gregorian, Ref, types::MonthCode};
use std::borrow::Cow;

pub(crate) fn full_year_pattern(pattern: &str) -> Result<Cow<'_, str>, NlsError32> {
    let mut output = None;
    let mut copied = 0;
    let mut quoted = false;
    let mut chars = pattern.char_indices().peekable();
    while let Some((at, scalar)) = chars.next() {
        if scalar == '\'' {
            if chars.peek().is_some_and(|(_, next)| *next == '\'') {
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if !quoted && scalar == 'y' {
            let mut end = at + 1;
            while chars.peek().is_some_and(|(_, next)| *next == 'y') {
                end = chars.next().unwrap().0 + 1;
            }
            if end - at < 4 {
                if output.is_none() {
                    let mut text = String::new();
                    text.try_reserve_exact(
                        pattern.len().checked_mul(4).ok_or(NlsError32::Capacity)?,
                    )
                    .map_err(|_| NlsError32::Allocation)?;
                    output = Some(text);
                }
                let text = output.as_mut().unwrap();
                text.push_str(&pattern[copied..at]);
                text.push_str("yyyy");
                copied = end;
            }
        }
    }
    Ok(if let Some(mut output) = output {
        output.push_str(&pattern[copied..]);
        Cow::Owned(output)
    } else {
        Cow::Borrowed(pattern)
    })
}

pub(super) fn validated(time: SystemTime32) -> Result<SystemTime32, NlsError32> {
    if !(100..=9999).contains(&time.year) || !(1..=12).contains(&time.month) || time.day > 31 {
        return Err(NlsError32::InvalidParameter);
    }
    let date = Date::try_new_gregorian(i32::from(time.year), time.month as u8, time.day as u8)
        .map_err(|_| NlsError32::InvalidParameter)?;
    Ok(SystemTime32 {
        day_of_week: date.day_of_week() as u16 % 7,
        ..time
    })
}

pub(crate) fn calendar_to_gregorian(
    time: SystemTime32,
    calendar: u32,
) -> Result<SystemTime32, NlsError32> {
    let calendar = CalendarId32::from_id(calendar)?;
    let year = i32::from(time.year)
        - if calendar == CalendarId32::Korean {
            2333
        } else {
            0
        };
    let cal = calendar.calendar();
    let first =
        Date::try_new_from_codes(None, year, MonthCode::new_normal(1).unwrap(), 1, Ref(&cal))
            .map_err(|_| NlsError32::InvalidParameter)?;
    if time.month == 0 || time.month > u16::from(first.months_in_year()) || time.day > 31 {
        return Err(NlsError32::InvalidParameter);
    }
    let month = time.month as u8;
    let code = if calendar == CalendarId32::Hebrew && first.months_in_year() == 13 {
        if month == 6 {
            MonthCode::new_leap(5)
        } else {
            MonthCode::new_normal(month - u8::from(month > 6))
        }
    } else {
        MonthCode::new_normal(month)
    }
    .ok_or(NlsError32::InvalidParameter)?;
    let date = Date::try_new_from_codes(None, year, code, time.day as u8, cal)
        .map_err(|_| NlsError32::InvalidParameter)?
        .to_calendar(Gregorian);
    let year = date.extended_year();
    if !(100..=9999).contains(&year) {
        return Err(NlsError32::InvalidParameter);
    }
    Ok(SystemTime32 {
        year: year as u16,
        month: u16::from(date.month().ordinal),
        day: u16::from(date.day_of_month().0),
        day_of_week: date.day_of_week() as u16 % 7,
        ..time
    })
}

pub(crate) fn calendar_year(time: SystemTime32, calendar: u32) -> Result<u16, NlsError32> {
    let calendar = CalendarId32::from_id(calendar)?;
    let time = validated(time)?;
    let date = Date::try_new_gregorian(i32::from(time.year), time.month as u8, time.day as u8)
        .map_err(|_| NlsError32::InvalidParameter)?
        .to_calendar(calendar.calendar());
    u16::try_from(
        date.extended_year()
            + if calendar == CalendarId32::Korean {
                2333
            } else {
                0
            },
    )
    .map_err(|_| NlsError32::InvalidParameter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automation_full_year_pattern_preserves_literals_and_borrows_unchanged_patterns() {
        assert_eq!(
            full_year_pattern("dd.MM.yy 'yy' yyyy''M").unwrap(),
            "dd.MM.yyyy 'yy' yyyy''M"
        );
        assert!(matches!(
            full_year_pattern("yyyy 'yy' M/d").unwrap(),
            Cow::Borrowed(_)
        ));
    }
}
