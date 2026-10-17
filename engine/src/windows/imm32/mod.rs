mod api;
mod context;
mod state;

pub use context::InputContexts32;
pub use state::{CandidateWindow32, CompositionWindow32, InputComposition32, InputContext32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmError32 {
    InvalidHandle,
    InvalidParameter,
    WrongThread,
    Associated,
    Capacity,
    Allocation,
    Exhausted,
}

#[cfg(test)]
mod tests;
pub use api::{ImmApi32, imm32_module};
