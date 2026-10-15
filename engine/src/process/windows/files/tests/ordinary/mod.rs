mod handles;
mod locks;
mod paths;
mod progress;
mod utilities;

use super::support;
use crate::process::windows::files::ordinary::FileCallOutcome32;

fn returned(outcome: FileCallOutcome32) -> (u32, Option<u32>) {
    match outcome {
        FileCallOutcome32::Return { value, last_error } => (value, last_error),
        _ => panic!("expected completed ordinary call"),
    }
}
