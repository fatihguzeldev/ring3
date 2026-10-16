use super::{TICKS_PER_DAY, TICKS_PER_MILLISECOND};
use crate::windows::nls::NlsError32;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemTime32 {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millisecond: u16,
}
impl SystemTime32 {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let mut words = [0; 8];
        for (word, bytes) in words.iter_mut().zip(bytes.chunks_exact(2)) {
            *word = u16::from_le_bytes(bytes.try_into().unwrap());
        }
        Self {
            year: words[0],
            month: words[1],
            day_of_week: words[2],
            day: words[3],
            hour: words[4],
            minute: words[5],
            second: words[6],
            millisecond: words[7],
        }
    }
    pub fn bytes(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        for (at, value) in [
            self.year,
            self.month,
            self.day_of_week,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.millisecond,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[at * 2..at * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}
fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
fn days(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let y = year - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let d = (153 * m + 2) / 5 + day - 1;
    era * 146097 + y * 365 + y / 4 - y / 100 + d
}
const EPOCH_DAYS: i64 = 584_694;
pub fn to_filetime(time: SystemTime32) -> Result<u64, NlsError32> {
    if !(1601..=30827).contains(&time.year)
        || !(1..=12).contains(&time.month)
        || time.hour > 23
        || time.minute > 59
        || time.second > 59
        || time.millisecond > 999
    {
        return Err(NlsError32::InvalidParameter);
    }
    let limit = match time.month {
        2 => 28 + u16::from(leap(i64::from(time.year))),
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if time.day == 0 || time.day > limit {
        return Err(NlsError32::InvalidParameter);
    }
    let d = days(
        i64::from(time.year),
        i64::from(time.month),
        i64::from(time.day),
    ) - EPOCH_DAYS;
    let millis =
        (u64::from(time.hour) * 3600 + u64::from(time.minute) * 60 + u64::from(time.second)) * 1000
            + u64::from(time.millisecond);
    (d as u64)
        .checked_mul(TICKS_PER_DAY)
        .and_then(|value| value.checked_add(millis * TICKS_PER_MILLISECOND))
        .filter(|&value| value < i64::MAX as u64 + 1)
        .ok_or(NlsError32::InvalidParameter)
}
pub fn from_filetime(value: u64) -> Result<SystemTime32, NlsError32> {
    if value >= 1u64 << 63 {
        return Err(NlsError32::InvalidParameter);
    }
    let day = value / TICKS_PER_DAY;
    let z = day as i64 + EPOCH_DAYS;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let millis = (value % TICKS_PER_DAY) / TICKS_PER_MILLISECOND;
    Ok(SystemTime32 {
        year: year as u16,
        month: month as u16,
        day: d as u16,
        day_of_week: ((day + 1) % 7) as u16,
        hour: (millis / 3_600_000) as u16,
        minute: (millis / 60_000 % 60) as u16,
        second: (millis / 1000 % 60) as u16,
        millisecond: (millis % 1000) as u16,
    })
}
pub fn shift_filetime(value: u64, minutes: i32) -> Result<u64, NlsError32> {
    let shifted = i128::from(value) + i128::from(minutes) * 600_000_000;
    if !(0..1i128 << 63).contains(&shifted) {
        return Err(NlsError32::InvalidParameter);
    }
    Ok(shifted as u64)
}
