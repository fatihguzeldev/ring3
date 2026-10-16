mod model;
mod table;

pub(crate) use model::{AcceleratorHandle32, AcceleratorStore32, ResourceKey32};
pub(crate) use table::Accelerator32;

#[cfg(test)]
mod tests;
