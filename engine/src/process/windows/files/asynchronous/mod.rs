mod acceptance;
mod calls;
mod completion;
mod context;
mod locks;
mod positioned;

pub(super) use context::{AsyncProgress32, AsyncRuntime32};

#[cfg(test)]
mod tests;

fn synchronization_error(
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
