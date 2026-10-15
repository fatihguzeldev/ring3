mod coherence;
mod lifetime;
mod reservations;

use crate::{files::FileError32, process::windows::files::ordinary::FileCallOutcome32};
fn returned(outcome: Result<FileCallOutcome32, FileError32>) -> u32 {
    match outcome.unwrap() {
        FileCallOutcome32::Return { value, .. } => value,
        _ => panic!("owned test bytes must be ready"),
    }
}
