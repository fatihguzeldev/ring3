mod static_fields;
pub use static_fields::{locale_number, static_information};
mod calendar;
mod decimal;
mod information;
mod names;
mod provider;
mod regions;
use super::{LocaleIdentity32, locale_identity};
use crate::windows::nls::NlsError32;
pub use calendar::CalendarNames32;
pub(in crate::windows::nls) use calendar::windows_pattern;
use decimal::DecimalData32;
use names::Names32;
use regions::{RegionData32, region_data};
use std::sync::OnceLock;

#[derive(Debug)]
pub struct LocaleData32 {
    identity: LocaleIdentity32,
    region_name: String,
    script: String,
    region: RegionData32,
    localized: Names32,
    english: Names32,
    native: Names32,
    pub calendar: CalendarNames32,
    locale: icu_locale::Locale,
    decimal: OnceLock<DecimalData32>,
    english_currency: OnceLock<String>,
    native_currency: OnceLock<String>,
}
impl LocaleData32 {
    pub fn new(lcid: u32, display_lcid: u32) -> Result<Self, NlsError32> {
        let identity = locale_identity(lcid)?;
        let locale: icu_locale::Locale = identity
            .language_tag()
            .parse()
            .map_err(|_| NlsError32::InvalidParameter)?;
        let display: icu_locale::Locale = locale_identity(display_lcid)?
            .language_tag()
            .parse()
            .map_err(|_| NlsError32::InvalidParameter)?;
        let english: icu_locale::Locale = "en".parse().unwrap();
        let region_name = locale
            .id
            .region
            .map(|region| region.to_string())
            .unwrap_or_default();
        let region = region_data(&region_name);
        let mut language = locale.id.clone();
        icu_locale::LocaleExpander::new_extended().maximize(&mut language);
        let script = language
            .script
            .map(|value| value.to_string())
            .unwrap_or_default();
        let mut calendar = CalendarNames32::new(&locale)?;
        if region.calendar != 1 {
            let patterns =
                crate::windows::nls::calendar::calendar_patterns(lcid, u32::from(region.calendar))?;
            calendar.short_date = patterns.0;
            calendar.long_date = patterns.1;
            calendar.year_month = patterns.2;
            calendar.month_day = crate::windows::nls::calendar::calendar_month_day_pattern(
                lcid,
                u32::from(region.calendar),
            )?;
        }
        Ok(Self {
            identity,
            region_name,
            script,
            region,
            localized: Names32::new(&locale, &display),
            english: Names32::new(&locale, &english),
            native: Names32::new(&locale, &locale),
            calendar,
            locale,
            decimal: OnceLock::new(),
            english_currency: OnceLock::new(),
            native_currency: OnceLock::new(),
        })
    }
    fn decimal(&self) -> Result<&DecimalData32, NlsError32> {
        cached(&self.decimal, || {
            DecimalData32::new(&self.locale, self.region.currency)
        })
    }
    fn currency_name(&self, english: bool) -> Result<&str, NlsError32> {
        let slot = if english {
            &self.english_currency
        } else {
            &self.native_currency
        };
        cached(slot, || {
            let locale = if english {
                "en".parse().unwrap()
            } else {
                self.locale.clone()
            };
            decimal::currency_name(&locale, self.region.currency)
        })
        .map(String::as_str)
    }
}

fn cached<T>(
    slot: &OnceLock<T>,
    create: impl FnOnce() -> Result<T, NlsError32>,
) -> Result<&T, NlsError32> {
    if let Some(value) = slot.get() {
        return Ok(value);
    }
    let value = create()?;
    let _ = slot.set(value);
    Ok(slot.get().expect("successful locale facet publication"))
}
