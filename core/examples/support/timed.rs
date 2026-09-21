use std::time::Duration;

use ring3_core::execution::{ClockError, Process32, ProcessResult, ProcessStop, StopReason};

#[derive(Debug)]
pub(crate) struct ClockFailure {
    pub error: ClockError,
    pub completed: ProcessResult,
}

pub(crate) fn run(
    process: &mut Process32,
    limit: u64,
    mut now: impl FnMut() -> Duration,
) -> Result<ProcessResult, ClockFailure> {
    let mut result = process.run(0);
    for _ in 0..limit {
        if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
            break;
        }
        if let Err(error) = process.set_elapsed_time(now()) {
            return Err(ClockFailure {
                error,
                completed: result,
            });
        }
        let step = process.run(1);
        result.instructions += step.instructions;
        result.api_calls += step.api_calls;
        result.reason = step.reason;
    }
    Ok(result)
}
