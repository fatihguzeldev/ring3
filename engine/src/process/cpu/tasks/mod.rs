mod config;
mod context;
mod control;
mod execution;
mod save;
mod transition;
pub(in crate::process) use control::TaskControlAuthority;
pub use transition::{CommittedTaskReturn32, TaskReturnTransition32};
mod request;
mod returning;

use crate::cpu::x86::tasks::TaskRegister;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct TaskReturnSources {
    current: TaskRegister,
    gdt_limit: u16,
}

pub use crate::cpu::x86::tasks::TaskReturnAdmission;
