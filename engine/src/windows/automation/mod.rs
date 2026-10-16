mod arrays;
mod catalog;
mod strings;
mod types;

pub(crate) use arrays::{ArrayBound32, ArrayLayout32};
pub use catalog::{Oleaut32Api32, oleaut32_module};
pub(crate) use strings::bstr_bytes;
pub use types::{AutomationError32, AutomationLimits32};

#[cfg(test)]
mod tests;
