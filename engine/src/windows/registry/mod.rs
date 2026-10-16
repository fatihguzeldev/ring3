mod catalog;
mod compound;
mod context;
mod copy;
mod deletion;
mod model;
mod notifications;
mod overrides;
mod path;
mod persistence;
mod prepared;
mod queries;
mod transaction;
mod types;
pub use catalog::{Advapi32Api32, advapi32_module};
pub use types::{
    RegistryChangeId32, RegistryChangeKind32, RegistryConfig32, RegistryError32, RegistryKeyId32,
    RegistryLimits32, RegistryView32,
};

pub use context::Registry32;
pub use model::{
    RegistryChange32, RegistryEntry32, RegistryKeyInfo32, RegistryNamedValue32, RegistryValue32,
};

pub use prepared::PreparedRegistry32;

pub use persistence::RegistrySeed32;
#[cfg(test)]
mod tests;

pub use notifications::RegistryWatch32;
