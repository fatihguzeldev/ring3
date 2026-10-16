mod state;
mod template;
pub(crate) use state::WindowDialog32;
pub(crate) use template::{DialogFont32, DialogName32, DialogTemplate32, DialogUnits32};

#[cfg(test)]
pub(crate) use template::DialogItem32;

#[cfg(test)]
mod tests;
