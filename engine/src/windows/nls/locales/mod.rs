mod inventory;
mod names;
pub use names::{installed_locales, locale_from_name, locale_is_neutral, locale_to_name};
pub(in crate::windows::nls) mod data;
mod identity;
pub use data::{CalendarNames32, LocaleData32, locale_number, static_information};
pub use identity::{LocaleIdentity32, locale_identity};
