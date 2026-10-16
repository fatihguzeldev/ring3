use crate::windows::nls::{
    NlsError32,
    calendar::{SystemTime32, from_filetime, shift_filetime, to_filetime},
};
#[derive(Clone, Copy)]
pub(super) struct TimeZone32 {
    pub bias: i32,
    standard: SystemTime32,
    standard_bias: i32,
    daylight: SystemTime32,
    daylight_bias: i32,
}
impl TimeZone32 {
    pub(super) fn fixed(bias: i32) -> Self {
        Self {
            bias,
            standard: SystemTime32::default(),
            standard_bias: 0,
            daylight: SystemTime32::default(),
            daylight_bias: 0,
        }
    }
    pub(super) fn read(bytes: [u8; 172]) -> Result<Self, NlsError32> {
        let value = Self {
            bias: i32::from_le_bytes(bytes[..4].try_into().unwrap()),
            standard: SystemTime32::from_bytes(bytes[68..84].try_into().unwrap()),
            standard_bias: i32::from_le_bytes(bytes[84..88].try_into().unwrap()),
            daylight: SystemTime32::from_bytes(bytes[152..168].try_into().unwrap()),
            daylight_bias: i32::from_le_bytes(bytes[168..172].try_into().unwrap()),
        };
        if value.standard.month == 0 && value.daylight.month == 0 {
            return Ok(value);
        }
        if value.standard.month == 0 || value.daylight.month == 0 {
            return Err(NlsError32::InvalidParameter);
        }
        value.transition(value.standard, 2000)?;
        value.transition(value.daylight, 2000)?;
        Ok(value)
    }
    fn transition(self, mut date: SystemTime32, year: u16) -> Result<u64, NlsError32> {
        if date.year == 0 {
            if !(1..=5).contains(&date.day) || date.day_of_week > 6 {
                return Err(NlsError32::InvalidParameter);
            }
            date.year = year;
            let week = date.day;
            date.day = 1;
            let first = from_filetime(to_filetime(date)?)?;
            let day = 1 + (date.day_of_week + 7 - first.day_of_week) % 7 + (week - 1) * 7;
            date.day = day;
            if to_filetime(date).is_err() && week == 5 {
                date.day -= 7;
            }
        }
        to_filetime(date)
    }
    fn total_bias(self, daylight: bool) -> Result<i32, NlsError32> {
        self.bias
            .checked_add(if daylight {
                self.daylight_bias
            } else {
                self.standard_bias
            })
            .ok_or(NlsError32::InvalidParameter)
    }
    fn daylight_at(self, value: u64) -> Result<bool, NlsError32> {
        if self.standard.month == 0 {
            return Ok(false);
        }
        let year = from_filetime(value)?.year;
        let start = shift_filetime(
            self.transition(self.daylight, year)?,
            self.total_bias(false)?,
        )?;
        let end = shift_filetime(
            self.transition(self.standard, year)?,
            self.total_bias(true)?,
        )?;
        Ok(if start < end {
            value >= start && value < end
        } else {
            value >= start || value < end
        })
    }
    pub(super) fn to_local(self, value: u64) -> Result<u64, NlsError32> {
        let bias = self.total_bias(self.daylight_at(value)?)?;
        shift_filetime(
            value,
            bias.checked_neg().ok_or(NlsError32::InvalidParameter)?,
        )
    }
    pub(super) fn to_utc(self, local: u64) -> Result<u64, NlsError32> {
        let standard = shift_filetime(local, self.total_bias(false)?)?;
        if self.to_local(standard)? == local {
            return Ok(standard);
        }
        if self.standard.month != 0 {
            let daylight = shift_filetime(local, self.total_bias(true)?)?;
            if self.to_local(daylight)? == local {
                return Ok(daylight);
            }
        }
        Err(NlsError32::InvalidParameter)
    }
    pub(super) fn bytes(self) -> [u8; 172] {
        let mut bytes = [0; 172];
        bytes[..4].copy_from_slice(&self.bias.to_le_bytes());
        for (index, unit) in "UTC".encode_utf16().enumerate() {
            bytes[4 + index * 2..6 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }
}
