use super::{SystemTime32, from_filetime, to_filetime};
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
    let time = from_filetime(to_filetime(time)?)?;
    format(time, pattern, true)
}
pub fn format_time(time: SystemTime32, pattern: &str) -> Result<String, NlsError32> {
    if time.hour > 23 || time.minute > 59 || time.second > 59 {
        return Err(NlsError32::InvalidParameter);
    }
    format(time, pattern, false)
}
fn format(time: SystemTime32, pattern: &str, date: bool) -> Result<String, NlsError32> {
    let mut out = String::new();
    out.try_reserve(pattern.len().saturating_mul(8))
        .map_err(|_| NlsError32::Allocation)?;
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                out.push('\'');
                chars.next();
            } else {
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
        let value = match (date, c, n) {
            (true, 'd', 1) => time.day.to_string(),
            (true, 'd', 2) => format!("{:02}", time.day),
            (true, 'd', 3) => DAYS[time.day_of_week as usize][..3].into(),
            (true, 'd', _) => DAYS[time.day_of_week as usize].into(),
            (true, 'M', 1) => time.month.to_string(),
            (true, 'M', 2) => format!("{:02}", time.month),
            (true, 'M', 3) => MONTHS[time.month as usize - 1][..3].into(),
            (true, 'M', _) => MONTHS[time.month as usize - 1].into(),
            (true, 'y', 1) => format!("{}", time.year % 100),
            (true, 'y', 2) => format!("{:02}", time.year % 100),
            (true, 'y', _) => format!("{:04}", time.year),
            (true, 'g', _) => "A.D.".into(),
            (false, 'H', 1) => time.hour.to_string(),
            (false, 'H', _) => format!("{:02}", time.hour),
            (false, 'h', 1) => (if time.hour % 12 == 0 {
                12
            } else {
                time.hour % 12
            })
            .to_string(),
            (false, 'h', _) => format!(
                "{:02}",
                if time.hour % 12 == 0 {
                    12
                } else {
                    time.hour % 12
                }
            ),
            (false, 'm', 1) => time.minute.to_string(),
            (false, 'm', _) => format!("{:02}", time.minute),
            (false, 's', 1) => time.second.to_string(),
            (false, 's', _) => format!("{:02}", time.second),
            (false, 't', 1) => if time.hour < 12 { "A" } else { "P" }.into(),
            (false, 't', _) => if time.hour < 12 { "AM" } else { "PM" }.into(),
            _ => c.to_string().repeat(n),
        };
        out.push_str(&value);
    }
    if quoted {
        return Err(NlsError32::InvalidParameter);
    }
    Ok(out)
}
