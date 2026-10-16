mod activation;
mod calls;
mod cleanup;
mod context;
mod invocation;
mod lifetime;
mod proxy;
mod rundown;

pub(super) use context::RemoteActivation32;
pub(super) use context::{ExecutionApartment32, MarshalingRuntime32, Remote32};
pub(in crate::process) use lifetime::PreparedComThreadExit32;

#[cfg(test)]
mod tests;
