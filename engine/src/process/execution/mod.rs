#![forbid(unsafe_code)]

pub(super) mod authority;
pub(super) mod compiled;
pub(super) mod installation;
pub(super) mod resident;

#[cfg(test)]
mod installed_resident_disposal_tests;
