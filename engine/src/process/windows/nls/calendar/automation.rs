use crate::{
    process::{EngineInstance, HostError},
    windows::nls::calendar::{
        SystemTime32, calendar_patterns, format_civil_calendar_date, format_civil_date_with_names,
        format_time_with_names, full_year_pattern,
    },
};

impl EngineInstance {
    pub(in crate::process) fn windows_calendar_information(
        &mut self,
        locale: u32,
        calendar: u32,
        kind: u32,
    ) -> Result<(String, Option<u32>), HostError> {
        self.memory()?;
        let locale = self.nls.profile.locale(locale).map_err(HostError::Nls)?;
        let data = self.nls.data(locale).map_err(HostError::Nls)?;
        if calendar != data.calendar_id() && matches!(kind, 5 | 6 | 0x2f) {
            let (short, long, year_month) =
                calendar_patterns(locale, calendar).map_err(HostError::Nls)?;
            return Ok((
                match kind {
                    5 => short,
                    6 => long,
                    _ => year_month,
                },
                None,
            ));
        }
        data.calendar_information(calendar, kind)
            .map_err(HostError::Nls)
    }

    pub(in crate::process) fn windows_civil_date_text(
        &mut self,
        locale: u32,
        calendar: u32,
        time: SystemTime32,
        date: bool,
        clock: bool,
    ) -> Result<String, HostError> {
        self.memory()?;
        let locale = self.nls.profile.locale(locale).map_err(HostError::Nls)?;
        let data = self.nls.data(locale).map_err(HostError::Nls)?;
        let mut text = if date {
            let alternate;
            let pattern = if calendar == data.calendar_id() {
                &data.calendar.short_date
            } else {
                alternate = calendar_patterns(locale, calendar)
                    .map_err(HostError::Nls)?
                    .0;
                &alternate
            };
            let pattern = full_year_pattern(pattern).map_err(HostError::Nls)?;
            if calendar == 1 {
                format_civil_date_with_names(time, &pattern, &data.calendar)
            } else {
                format_civil_calendar_date(time, &pattern, locale, calendar)
            }
            .map_err(HostError::Nls)?
        } else {
            String::new()
        };
        if clock {
            let clock = format_time_with_names(time, &data.calendar.time, &data.calendar)
                .map_err(HostError::Nls)?;
            text.try_reserve(clock.len() + usize::from(date))
                .map_err(|_| HostError::Nls(crate::windows::nls::NlsError32::Allocation))?;
            if date {
                text.push(' ');
            }
            text.push_str(&clock);
        }
        Ok(text)
    }
}
