//! protected-mode task-return foundation. admission is not a task-switch commit.

mod authority;
mod outcome;
mod record;
mod register;
mod returning;
mod save;

pub use authority::{TaskAuthority, TaskDescriptorTable};
pub use outcome::TaskReturnAdmission;
pub use record::{
    TaskFormat, TaskRecord, TaskSave, TaskState, TaskState16, TaskState32, encode_return,
};
pub use register::TaskRegister;
pub use returning::{PreparedTaskReturn, prepare_return};
pub use save::{
    MAX_TASK_SAVE_WORDS, TaskSaveOverlayAuthority, TaskSaveOverlayError, TaskSavePlan, TaskSaveWord,
};

/// missing trusted source bytes are distinct from an explicitly invalid guest image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskReturnError {
    MissingAuthority,
    InvalidTss(u32),
    NotPresent(u32),
}
