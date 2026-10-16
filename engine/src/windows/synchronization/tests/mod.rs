mod events;
mod objects;
mod pseudo;
mod timers;
mod waits;

use super::*;
fn context() -> SynchronizationContext32 {
    SynchronizationContext32::new(
        7,
        SynchronizationLimits32 {
            events: 8,
            handles: 16,
            bindings: 8,
            waits: 8,
        },
        0,
    )
    .unwrap()
}
fn ready(outcome: WaitOutcome32) -> u32 {
    let WaitOutcome32::Ready(result) = outcome else {
        panic!("wait must finish")
    };
    assert_eq!(result.last_error, None);
    result.value
}
fn pending(outcome: WaitOutcome32) -> WaitToken32 {
    let WaitOutcome32::Pending(token) = outcome else {
        panic!("wait must suspend")
    };
    token
}
