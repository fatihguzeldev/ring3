use crate::{
    process::{EngineInstance, HostError},
    windows::{
        automation::{
            AutomationError32 as E,
            variants::{Date32, DateSymbols32, Variant32, parse_date},
        },
        nls::calendar::{calendar_to_gregorian, calendar_year, from_filetime},
        text::ordinal_key,
    },
};

impl EngineInstance {
    fn automation_calendar(&mut self, locale: u32, flags: u16) -> Result<(u32, u32), HostError> {
        let locale = self.automation_locale_information(locale, 1)?.1.unwrap();
        let calendar = match flags & 0x68 {
            0 => self
                .automation_locale_information(locale, 0x1009)?
                .1
                .unwrap(),
            8 => 6,
            0x20 => 7,
            0x40 => 1,
            _ => return Err(HostError::Automation(E::InvalidParameter)),
        };
        Ok((locale, calendar))
    }

    pub(super) fn automation_date_text(
        &mut self,
        source: Variant32,
        locale: u32,
        flags: u16,
    ) -> Result<Vec<u8>, HostError> {
        let date =
            Date32::from_extended(source.number().map_err(HostError::Automation)?.extended())
                .map_err(HostError::Automation)?;
        let (locale, calendar) = self.automation_calendar(locale, flags)?;
        let text = self
            .windows_civil_date_text(
                locale,
                calendar,
                date.system_time().map_err(HostError::Automation)?,
                date.days != 0,
                date.days == 0 || date.seconds != 0,
            )
            .map_err(super::coercion::locale_error)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(
                text.len()
                    .checked_mul(2)
                    .ok_or(HostError::Automation(E::Capacity))?,
            )
            .map_err(|_| HostError::Automation(E::Allocation))?;
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        Ok(bytes)
    }

    pub(super) fn automation_parse_date(
        &mut self,
        units: &[u16],
        locale: u32,
        flags: u16,
    ) -> Result<Variant32, HostError> {
        let (locale, calendar) = self.automation_calendar(locale, flags)?;
        let information = |engine: &mut Self, kind| {
            engine
                .windows_calendar_information(locale, calendar, kind)
                .map_err(super::coercion::locale_error)
        };
        let pattern = information(self, 5)?.0;
        let mut quoted = false;
        let order = pattern
            .chars()
            .find_map(|scalar| {
                if scalar == '\'' {
                    quoted = !quoted;
                }
                if quoted {
                    return None;
                }
                match scalar {
                    'd' => Some(1),
                    'M' => Some(0),
                    'y' => Some(2),
                    _ => None,
                }
            })
            .unwrap_or(0);
        let mut months = Vec::new();
        let mut weekdays = Vec::new();
        months
            .try_reserve_exact(52)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        weekdays
            .try_reserve_exact(14)
            .map_err(|_| HostError::Automation(E::Allocation))?;
        for genitive in [0, 0x10000000] {
            for kind in 21..=46 {
                let name = information(self, kind | genitive)?.0;
                if !name.is_empty() {
                    let month = (kind - if kind >= 34 { 34 } else { 21 } + 1) as u16;
                    months.push((
                        ordinal_key(
                            &name
                                .trim_end_matches('.')
                                .encode_utf16()
                                .collect::<Vec<_>>(),
                        ),
                        month,
                    ));
                }
            }
        }
        for kind in 7..=20 {
            let name = information(self, kind)?.0;
            weekdays.push(ordinal_key(
                &name
                    .trim_end_matches('.')
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            ));
        }
        let current = from_filetime(self.process_creation_filetime()?)
            .map_err(|_| HostError::Automation(E::InvalidParameter))?;
        let symbols = DateSymbols32 {
            order,
            separator: self.automation_locale_information(locale, 0x1d)?.0,
            am: self.automation_locale_information(locale, 0x28)?.0,
            pm: self.automation_locale_information(locale, 0x29)?.0,
            months,
            weekdays,
            current_year: Some(
                calendar_year(current, calendar)
                    .map_err(|_| HostError::Automation(E::TypeMismatch))?,
            ),
            year_max: information(self, 0x30)?.1.unwrap() as u16,
        };
        let parsed = parse_date(units, &symbols).map_err(HostError::Automation)?;
        let time = if parsed.date {
            calendar_to_gregorian(parsed.time, calendar)
                .map_err(|_| HostError::Automation(E::TypeMismatch))?
        } else {
            parsed.time
        };
        let date = Date32::from_system_time(time).map_err(HostError::Automation)?;
        let (bits, _, _) = date
            .extended()
            .to_ieee(11, 52, crate::math::Rounding::Nearest);
        Ok(Variant32::new(7, &bits.to_le_bytes()))
    }
}
