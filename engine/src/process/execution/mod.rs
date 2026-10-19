#![forbid(unsafe_code)]

pub(super) mod authority;
pub(super) mod cache;
#[cfg(any(all(feature = "observability", target_arch = "wasm32"), test))]
pub(super) mod census;
pub(super) mod compiled;
pub(super) mod continuation;
pub(super) mod installation;
#[cfg(any(target_arch = "wasm32", test))]
pub(super) mod native_success;
pub(super) mod owned;
pub(super) mod receipt;
pub(super) mod resident;
mod stopped;

#[cfg(test)]
#[path = "owned/tests.rs"]
mod owned_tests;

#[cfg(test)]
#[path = "installation/disposal/tests.rs"]
mod installed_resident_disposal_tests;

#[cfg(test)]
#[path = "census/tests.rs"]
mod census_tests;
