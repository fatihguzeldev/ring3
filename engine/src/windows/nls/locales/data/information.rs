use super::LocaleData32;
use crate::windows::nls::NlsError32;
impl LocaleData32 {
    pub fn information(&self, kind: u32) -> Result<(String, Option<u32>), NlsError32> {
        let genitive = kind & 0x10000000 != 0;
        let kind = kind & 0xffff;
        if genitive && !matches!(kind, 0x38..=0x4f | 0x100e | 0x100f) {
            return Err(NlsError32::InvalidFlags);
        }
        if let Some(value) = super::static_fields::static_information(self.identity.lcid, kind)? {
            return Ok(value);
        }
        let mut numeric = None;
        let c = &self.calendar;
        let text = match kind {
            1 | 9 => {
                numeric = Some(self.identity.lcid & 0xffff);
                format!("{:04x}", self.identity.lcid & 0xffff)
            }
            2 => self.localized.display()?.to_owned(),
            3 => self.identity.windows_three_letter.into(),
            4 => self.native.language()?.to_owned(),
            5 | 0xa => return number(self.region.dialing),
            6 => self.localized.region()?.to_owned(),
            7 => self.region.alpha3.into(),
            8 => self.native.region()?.to_owned(),
            0xb => return number(self.oem()),
            0xc => if self.decimal()?.decimal == "," {
                ";"
            } else {
                ","
            }
            .into(),
            0xd => return number(u32::from(!self.region.metric)),
            0xe | 0x16 => self.decimal()?.decimal.clone(),
            0xf | 0x17 => self.decimal()?.group.clone(),
            0x10 | 0x18 => self.decimal()?.grouping.clone(),
            0x11 => return number(2),
            0x12 => return number(1),
            0x13 => self.decimal()?.digits.clone(),
            0x14 => self.decimal()?.currency.clone(),
            0x15 => self.region.currency.into(),
            0x19 | 0x1a => return number(u32::from(self.region.currency_digits)),
            0x1b => return number(self.decimal()?.currency_order),
            0x1c => {
                return number(match self.decimal()?.currency_order {
                    0 => 1,
                    1 => 5,
                    2 => 9,
                    _ => 8,
                });
            }
            0x1d => separator(&c.short_date),
            0x1e => separator(&c.time),
            0x1f => c.short_date.clone(),
            0x20 => c.long_date.clone(),
            0x21 => return number(order(&c.short_date)),
            0x22 => return number(order(&c.long_date)),
            0x23 => return number(u32::from(c.time.contains('H'))),
            0x24 => return number(u32::from(c.short_date.contains("yyyy"))),
            0x25 => return number(u32::from(c.time.contains("HH") || c.time.contains("hh"))),
            0x26 => return number(u32::from(c.short_date.contains("dd"))),
            0x27 => return number(u32::from(c.short_date.contains("MM"))),
            0x28 => c.am.clone(),
            0x29 => c.pm.clone(),
            0x2a..=0x30 => c.days[(kind - 0x2a) as usize].clone(),
            0x31..=0x37 => c.abbreviated_days[(kind - 0x31) as usize].clone(),
            0x38..=0x43 => (if genitive {
                &c.genitive_months
            } else {
                &c.months
            })[(kind - 0x38) as usize]
                .clone(),
            0x44..=0x4f => (if genitive {
                &c.genitive_abbreviated_months
            } else {
                &c.abbreviated_months
            })[(kind - 0x44) as usize]
                .clone(),
            0x50 => String::new(),
            0x51 => self.decimal()?.negative.clone(),
            0x52 | 0x53 => return number(1),
            0x54 | 0x56 => {
                return number(u32::from(matches!(self.decimal()?.currency_order, 0 | 2)));
            }
            0x55 | 0x57 => return number(u32::from(self.decimal()?.currency_order >= 2)),
            0x59 => self.identity.language.into(),
            0x5a => self.region_name.clone(),
            0x5c => self.identity.windows_name().into(),
            0x5d => "H:mm:ss".into(),
            0x60..=0x66 => c.abbreviated_days[(kind - 0x60) as usize]
                .chars()
                .take(2)
                .collect(),
            0x67 => self.identity.language_three_letter.into(),
            0x68 => self.region.alpha3.into(),
            0x69 => "NaN".into(),
            0x6a => "∞".into(),
            0x6b => format!("{}∞", self.decimal()?.negative),
            0x6c => match self.script.as_str() {
                "Jpan" => "Hani;Hira;Kana;".into(),
                "Kore" => "Hani;Hang;".into(),
                "Hanb" => "Hani;Bopo;".into(),
                "Hans" | "Hant" => "Hani;".into(),
                _ => format!("{};", self.script),
            },
            0x6e => if self.identity.ansi == 0 {
                "en-US"
            } else {
                self.identity.name
            }
            .into(),
            0x6f => self.localized.language()?.to_owned(),
            0x70 => {
                return number(self.reading_layout());
            }
            0x71 => return number(u32::from(self.region_name.is_empty())),
            0x72 => self.english.display()?.to_owned(),
            0x73 => self.native.display()?.to_owned(),
            0x78 => c.month_day.clone(),
            0x79 => crate::windows::nls::calendar::time_pattern(&c.time, 2),
            0x7a => self.identity.windows_three_letter.into(),
            0x7b => self.identity.windows_name().into(),
            0x7c => c.long_date.clone(),
            0x7d => return number(0),
            0x7e => c.am.chars().take(1).collect(),
            0x7f => c.pm.chars().take(1).collect(),
            0x6d => self
                .identity
                .name
                .rsplit_once('-')
                .map_or("", |value| value.0)
                .into(),
            0x1001 => self.english.language()?.to_owned(),
            0x1002 => self.english.region()?.to_owned(),
            0x1003 => c.time.clone(),
            0x1004 => return number(self.identity.ansi),
            0x1005 => return number(u32::from(c.time.trim_start().starts_with("tt"))),
            0x1006 => c.year_month.clone(),
            0x1007 => self.currency_name(true)?.to_owned(),
            0x1008 => self.currency_name(false)?.to_owned(),
            0x1009 => return number(u32::from(self.region.calendar)),
            0x100e | 0x100f => String::new(),
            0x100a => return number(u32::from(self.region.paper)),
            0x100b => return number(self.alternate_calendar_id()),
            0x100c => return number(u32::from(self.region.first_day)),
            0x100d => return number(u32::from(self.region.first_week)),
            0x1010 => return number(1),
            0x1011 => return number(10000),
            0x1012 => return number(37),
            0x1014 => return number(1),
            0x1013 => self.identity.sort_name().into(),
            _ => return Err(NlsError32::InvalidFlags),
        };
        Ok((text, numeric))
    }
    pub fn calendar_information(
        &self,
        calendar: u32,
        kind: u32,
    ) -> Result<(String, Option<u32>), NlsError32> {
        let genitive = kind & 0x10000000 != 0;
        let kind = kind & 0xffff;
        if genitive && !matches!(kind, 21..=46) {
            return Err(NlsError32::InvalidFlags);
        }
        if calendar != 1 && kind == 0x38 {
            return Ok((
                crate::windows::nls::calendar::calendar_month_day_pattern(
                    self.identity.lcid,
                    calendar,
                )?,
                None,
            ));
        }
        if calendar != 1 && matches!(kind, 5 | 6 | 0x2f) {
            let value =
                crate::windows::nls::calendar::calendar_patterns(self.identity.lcid, calendar)?;
            return Ok((
                match kind {
                    5 => value.0,
                    6 => value.1,
                    _ => value.2,
                },
                None,
            ));
        }
        if calendar != 1 {
            return crate::windows::nls::calendar::calendar_text(
                self.identity.lcid,
                calendar,
                kind | if genitive { 0x10000000 } else { 0 },
            );
        }
        let c = &self.calendar;
        let text = match kind {
            1 => return number(calendar),
            2 => "Gregorian Calendar".into(),
            3 => return number(0),
            4 => c.era.clone(),
            5 => c.short_date.clone(),
            6 => c.long_date.clone(),
            7..=13 => c.days[(kind - 7) as usize].clone(),
            14..=20 => c.abbreviated_days[(kind - 14) as usize].clone(),
            21..=32 => (if genitive {
                &c.genitive_months
            } else {
                &c.months
            })[(kind - 21) as usize]
                .clone(),
            33 | 46 => String::new(),
            34..=45 => (if genitive {
                &c.genitive_abbreviated_months
            } else {
                &c.abbreviated_months
            })[(kind - 34) as usize]
                .clone(),
            0x2f => c.year_month.clone(),
            0x38 => c.month_day.clone(),
            0x30 => return number(2029),
            0x31..=0x37 => c.abbreviated_days[(kind - 0x31) as usize]
                .chars()
                .take(2)
                .collect(),
            _ => return Err(NlsError32::InvalidFlags),
        };
        Ok((text, None))
    }
    pub fn alternate_calendar_id(&self) -> u32 {
        super::static_fields::alternate_calendar(
            self.identity,
            &self.region_name,
            self.region.calendar,
        )
    }
    pub fn calendar_id(&self) -> u32 {
        u32::from(self.region.calendar)
    }
    pub fn reading_layout(&self) -> u32 {
        match self.script.as_str() {
            "Arab" | "Hebr" | "Syrc" | "Thaa" | "Nkoo" | "Adlm" | "Rohg" => 1,
            "Mong" => 2,
            _ => 0,
        }
    }
    pub fn oem(&self) -> u32 {
        super::static_fields::oem(self.identity.ansi, &self.region_name)
    }
}
fn number(value: u32) -> Result<(String, Option<u32>), NlsError32> {
    Ok((value.to_string(), Some(value)))
}
fn separator(pattern: &str) -> String {
    let mut quoted = false;
    for c in pattern.chars() {
        if c == '\'' {
            quoted = !quoted;
            continue;
        }
        if !quoted && !c.is_ascii_alphabetic() && !c.is_whitespace() {
            return c.to_string();
        }
    }
    String::new()
}
fn order(pattern: &str) -> u32 {
    let mut quoted = false;
    for c in pattern.chars() {
        if c == '\'' {
            quoted = !quoted;
            continue;
        }
        if !quoted {
            match c {
                'd' => return 1,
                'M' => return 0,
                'y' => return 2,
                _ => {}
            }
        }
    }
    0
}
