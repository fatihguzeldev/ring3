use crate::{
    math::{INVALID, OVERFLOW, Rounding, extended::Ext80},
    windows::{automation::AutomationError32 as E, nls::calendar::SystemTime32},
};
use icu_calendar::{Date, Gregorian};
mod parsing;
pub(crate) use parsing::{DateSymbols32, parse_date};

#[derive(Clone, Copy)]
pub(crate) struct Date32 {
    pub(crate) days: i32,
    pub(crate) seconds: u32,
}

fn epoch() -> Date<Gregorian> {
    Date::try_new_gregorian(1899, 12, 30).expect("OLE date epoch")
}
impl Date32 {
    pub(crate) fn from_extended(value: Ext80) -> Result<Self, E> {
        let (bits, flags, _) = value.to_ieee(11, 52, Rounding::Nearest);
        let number = f64::from_bits(bits);
        if flags & (INVALID | OVERFLOW) != 0 || !(-657435.0..2958466.0).contains(&number) {
            return Err(E::Overflow);
        }
        let (integer, flags, _) = value.to_integer(64, Rounding::Zero);
        if flags & INVALID != 0 {
            return Err(E::Overflow);
        }
        let mut days = integer as i64;
        let fraction = value
            .add(Ext80::from_i64(days).negated(), 0x037f)
            .value
            .absolute();
        let (seconds, flags, _) = fraction
            .multiply(Ext80::from_i64(86400), 0x037f)
            .value
            .to_integer(64, Rounding::Nearest);
        if flags & INVALID != 0 {
            return Err(E::Overflow);
        }
        let mut seconds = seconds as u32;
        if seconds == 86400 {
            seconds = 0;
            days += 1;
        }
        let result = Self {
            days: days as i32,
            seconds,
        };
        result.system_time()?;
        Ok(result)
    }

    pub(crate) fn system_time(self) -> Result<SystemTime32, E> {
        if self.seconds >= 86400 {
            return Err(E::InvalidParameter);
        }
        let date = Date::from_rata_die(epoch().to_rata_die() + i64::from(self.days), Gregorian);
        let year = date.extended_year();
        if !(100..=9999).contains(&year) {
            return Err(E::Overflow);
        }
        Ok(SystemTime32 {
            year: year as u16,
            month: u16::from(date.month().ordinal),
            day: u16::from(date.day_of_month().0),
            day_of_week: (date.day_of_week() as u16) % 7,
            hour: (self.seconds / 3600) as u16,
            minute: (self.seconds / 60 % 60) as u16,
            second: (self.seconds % 60) as u16,
            millisecond: 0,
        })
    }

    pub(crate) fn from_system_time(time: SystemTime32) -> Result<Self, E> {
        if !(100..=9999).contains(&time.year)
            || !(1..=12).contains(&time.month)
            || !(1..=31).contains(&time.day)
            || time.hour > 23
            || time.minute > 59
            || time.second > 59
        {
            return Err(E::TypeMismatch);
        }
        let date = Date::try_new_gregorian(i32::from(time.year), time.month as u8, time.day as u8)
            .map_err(|_| E::TypeMismatch)?;
        Ok(Self {
            days: (date.to_rata_die() - epoch().to_rata_die()) as i32,
            seconds: u32::from(time.hour) * 3600
                + u32::from(time.minute) * 60
                + u32::from(time.second),
        })
    }

    pub(crate) fn extended(self) -> Ext80 {
        let fraction = Ext80::from_i64(i64::from(self.seconds))
            .divide(Ext80::from_i64(86400), 0x037f)
            .value;
        Ext80::from_i64(i64::from(self.days))
            .add(
                if self.days < 0 {
                    fraction.negated()
                } else {
                    fraction
                },
                0x037f,
            )
            .value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn negative_ole_dates_use_the_absolute_fraction_and_round_trip_before_filetime_epoch() {
        let date = Date32::from_extended(Ext80::from_f64_bits((-1.25f64).to_bits())).unwrap();
        let time = date.system_time().unwrap();
        assert_eq!(
            (time.year, time.month, time.day, time.hour),
            (1899, 12, 29, 6)
        );
        let (bits, flags, _) =
            Date32::from_system_time(time)
                .unwrap()
                .extended()
                .to_ieee(11, 52, Rounding::Nearest);
        assert_eq!(flags & (INVALID | OVERFLOW), 0);
        assert_eq!(bits, (-1.25f64).to_bits());
        let minimum = Date32::from_extended(Ext80::from_f64_bits((-657434.0f64).to_bits()))
            .unwrap()
            .system_time()
            .unwrap();
        assert_eq!((minimum.year, minimum.month, minimum.day), (100, 1, 1));
        assert!(Date32::from_extended(Ext80::from_f64_bits((-657435.0f64).to_bits())).is_err());
    }
}
