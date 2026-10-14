//! protected-mode task-return foundation. admission is not a task-switch commit.

mod authority;
mod record;
mod register;
mod returning;

pub use authority::{TaskAuthority, TaskDescriptorTable};
pub use record::{
    TaskFormat, TaskRecord, TaskSave, TaskState, TaskState16, TaskState32, encode_return,
};
pub use register::TaskRegister;
pub use returning::{PreparedTaskReturn, prepare_return};

/// missing trusted source bytes are distinct from an explicitly invalid guest image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskReturnError {
    MissingAuthority,
    InvalidTss(u32),
    NotPresent(u32),
}
