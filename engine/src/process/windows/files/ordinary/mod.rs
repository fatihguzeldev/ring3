mod thread;
pub(in crate::process::windows::files) use thread::OrdinaryThreadReply32;
pub(super) mod access;
mod calls;
mod context;
mod environment;
mod error_mode;
pub use environment::FileEnvironment32;
mod handles;
mod locks;
pub(super) mod memory;
mod paths;
mod progress;
pub(in crate::process::windows::files) use progress::PreparedCopyThreadExit32;
mod search;

pub(super) use context::OrdinaryRuntime32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) enum FileCallOutcome32 {
    Return { value: u32, last_error: Option<u32> },
    Callback(crate::process::WindowsCallback32),
    Pending,
    PendingBytes(crate::files::FileReadRequest32),
}

impl FileCallOutcome32 {
    pub(super) const fn success(value: u32) -> Self {
        Self::Return {
            value,
            last_error: None,
        }
    }
    pub(super) const fn error(value: u32, code: u32) -> Self {
        Self::Return {
            value,
            last_error: Some(code),
        }
    }
}
