mod delivery;
mod marshaling;
mod publication;
mod remote;
mod types;
pub(super) use types::Dialog32;
pub use types::{User32DialogIcon32, User32DialogRequest32};

#[cfg(test)]
mod tests;

pub(in crate::process::windows::user32) mod resources;
