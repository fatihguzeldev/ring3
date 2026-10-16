use crate::windows::nls::NlsError32;
use icu_experimental::displaynames::{
    LanguageDisplayNames, LocaleDisplayNamesFormatter, RegionDisplayNames,
};

#[derive(Debug)]
pub(super) struct Names32 {
    pub language: String,
    pub region: String,
    pub display: String,
}
impl Names32 {
    pub(super) fn new(
        target: &icu_locale::Locale,
        display: &icu_locale::Locale,
    ) -> Result<Self, NlsError32> {
        let languages = LanguageDisplayNames::try_new(display.clone().into(), Default::default())
            .map_err(|_| NlsError32::Unsupported)?;
        let regions = RegionDisplayNames::try_new(display.clone().into(), Default::default())
            .map_err(|_| NlsError32::Unsupported)?;
        let formatter =
            LocaleDisplayNamesFormatter::try_new(display.clone().into(), Default::default())
                .map_err(|_| NlsError32::Unsupported)?;
        Ok(Self {
            language: languages
                .of(target.id.language)
                .unwrap_or(target.id.language.as_str())
                .into(),
            region: target
                .id
                .region
                .map(|value| regions.of(value).unwrap_or(value.as_str()).to_string())
                .unwrap_or_default(),
            display: formatter.of(target).into_owned(),
        })
    }
}
