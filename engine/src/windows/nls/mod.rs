#![forbid(unsafe_code)]
mod api;
pub mod calendar;
pub mod linguistics;
pub mod locales;
mod profile;
pub use api::NlsApi32;
pub use profile::{NlsError32, NlsProfile32};
#[cfg(test)]
mod tests;
