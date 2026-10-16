mod termination;
mod thread;
pub(in crate::process) use thread::FileThreadReply32;
mod acceptance;
mod blocking;
mod calls;
mod completion;
mod context;
mod locks;
mod positioned;

pub(super) use acceptance::capture_mappings;
pub(super) use completion::status;
pub(super) use context::{AsyncKind32, AsyncOperation32, AsyncStatus32, AsyncTarget32};
pub(super) use context::{AsyncProgress32, AsyncRuntime32};

#[cfg(test)]
mod tests;

pub(in crate::process::windows::files) fn synchronization_error(
    error: crate::windows::synchronization::SynchronizationError32,
) -> crate::files::FileError32 {
    use crate::{
        files::FileError32 as File, windows::synchronization::SynchronizationError32 as Sync,
    };
    match error {
        Sync::Allocation => File::Allocation,
        Sync::Capacity => File::Capacity,
        Sync::Exhausted => File::Exhausted,
        Sync::AccessDenied => File::AccessDenied,
        Sync::Unsupported => File::Unsupported,
        _ => File::InvalidHandle,
    }
}
