mod api;
mod bindings;
mod context;
mod events;
mod objects;
mod timers;
mod types;
mod waits;

pub use api::SynchronizationApi32;
pub use bindings::{PreparedEventBindings32, PreparedIoSignals32, PreparedTimerEvent32};
pub use context::SynchronizationContext32;
pub use events::{PreparedCloseHandles32, PreparedDuplicate32, PreparedEventShutdown32};
pub use objects::{PreparedMutexRelease32, PreparedObjectHandle32};
pub(crate) use timers::TimerApc32;
pub use types::{
    EVENT_ALL_ACCESS, EVENT_MODIFY_STATE, EventBinding32, INFINITE, MAX_WAIT_OBJECTS,
    MessageWait32, SYNCHRONIZE, SynchronizationError32, SynchronizationLimits32, WAIT_FAILED,
    WAIT_TIMEOUT, WaitMode32, WaitOutcome32, WaitResult32, WaitToken32,
};
pub use waits::{PreparedSignals32, WaitHandles32};

#[cfg(test)]
mod tests;
