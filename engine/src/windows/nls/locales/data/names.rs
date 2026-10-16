use super::cached;
use crate::windows::nls::NlsError32;
use icu_experimental::displaynames::{
    LanguageDisplayNames, LocaleDisplayNamesFormatter, RegionDisplayNames,
};
use std::sync::OnceLock;

#[derive(Debug)]
pub(super) struct Names32 {
    target: icu_locale::Locale,
    display: icu_locale::Locale,
    language: OnceLock<String>,
    region: OnceLock<String>,
    full: OnceLock<String>,
}
impl Names32 {
    pub(super) fn new(target: &icu_locale::Locale, display: &icu_locale::Locale) -> Self {
        Self {
            target: target.clone(),
            display: display.clone(),
            language: OnceLock::new(),
            region: OnceLock::new(),
            full: OnceLock::new(),
        }
    }
    pub(super) fn language(&self) -> Result<&str, NlsError32> {
        cached(&self.language, || {
            let names =
                LanguageDisplayNames::try_new(self.display.clone().into(), Default::default())
                    .map_err(|_| NlsError32::Unsupported)?;
            Ok(names
                .of(self.target.id.language)
                .unwrap_or(self.target.id.language.as_str())
                .to_owned())
        })
        .map(String::as_str)
    }
    pub(super) fn region(&self) -> Result<&str, NlsError32> {
        cached(&self.region, || {
            let Some(region) = self.target.id.region else {
                return Ok(String::new());
            };
            let names =
                RegionDisplayNames::try_new(self.display.clone().into(), Default::default())
                    .map_err(|_| NlsError32::Unsupported)?;
            Ok(names.of(region).unwrap_or(region.as_str()).to_owned())
        })
        .map(String::as_str)
    }
    pub(super) fn display(&self) -> Result<&str, NlsError32> {
        cached(&self.full, || {
            let formatter = LocaleDisplayNamesFormatter::try_new_unstable(
                &super::provider::DisplayProvider32 {
                    needs_variants: !self.target.id.variants.is_empty(),
                },
                self.display.clone().into(),
                Default::default(),
            )
            .map_err(|_| NlsError32::Unsupported)?;
            Ok(formatter.of(&self.target).into_owned())
        })
        .map(String::as_str)
    }
}
