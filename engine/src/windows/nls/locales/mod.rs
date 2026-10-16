mod inventory;
mod names;
mod identity;
pub use names::{installed_locales, locale_from_name, locale_is_neutral, locale_to_name};
pub use identity::{LocaleIdentity32, locale_identity};
