#![forbid(unsafe_code)]

pub(super) mod authority;
pub(super) mod cache;
pub(super) mod compiled;
pub(super) mod installation;
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
