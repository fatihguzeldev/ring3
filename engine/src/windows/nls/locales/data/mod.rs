mod static_fields;
pub use static_fields::{locale_number, static_information};
mod calendar;
mod decimal;
mod information;
mod names;
mod regions;
use super::{LocaleIdentity32, locale_identity};
use crate::windows::nls::NlsError32;
pub use calendar::CalendarNames32;
pub(in crate::windows::nls) use calendar::windows_pattern;
use decimal::DecimalData32;
use names::Names32;
use regions::{RegionData32, region_data};

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
    decimal: DecimalData32,
    english_currency: String,
    native_currency: String,
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
            localized: Names32::new(&locale, &display)?,
            english: Names32::new(&locale, &english)?,
            native: Names32::new(&locale, &locale)?,
            calendar,
            decimal: DecimalData32::new(&locale, region.currency)?,
            english_currency: decimal::currency_name(&english, region.currency)?,
            native_currency: decimal::currency_name(&locale, region.currency)?,
        })
    }
}
