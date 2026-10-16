mod calls;
mod com;
mod context;
mod enumeration;
mod host;
mod host_input;
pub use host_input::RegistryInput32;
mod lifecycle;
mod memory;
mod multiple;
mod mutations;
mod notifications;
mod opening;
mod output;
mod queries;
pub use crate::windows::registry::{
    RegistryChange32, RegistryChangeId32, RegistryConfig32, RegistryError32, RegistryLimits32,
    RegistrySeed32, RegistryView32,
};
pub(in crate::process) use context::{RegistryReply32, RegistryRuntime32};
pub use lifecycle::RegistryWait32;

#[cfg(test)]
mod tests;
