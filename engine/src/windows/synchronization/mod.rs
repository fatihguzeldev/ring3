mod bindings;
mod context;
mod events;
mod types;
mod waits;

pub use bindings::{PreparedEventBindings32, PreparedIoSignals32};
pub use context::SynchronizationContext32;
pub use events::{PreparedCloseHandles32, PreparedDuplicate32};
pub use types::{
    EVENT_ALL_ACCESS, EVENT_MODIFY_STATE, EventBinding32, INFINITE, MAX_WAIT_OBJECTS,
    MessageWait32, SYNCHRONIZE, SynchronizationError32, SynchronizationLimits32, WAIT_FAILED,
    WAIT_TIMEOUT, WaitMode32, WaitOutcome32, WaitResult32, WaitToken32,
};
pub use waits::{PreparedSignals32, WaitHandles32};

#[cfg(test)]
mod tests;
