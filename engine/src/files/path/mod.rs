mod context;
mod matching;
mod normalization;
pub(crate) use context::FilePathScope32;
pub(crate) use matching::matches;
pub(crate) use normalization::{fold, name, normalize, parent, root_len};

#[cfg(test)]
mod tests;
