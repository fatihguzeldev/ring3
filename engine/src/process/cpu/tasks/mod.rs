mod config;
mod request;
mod returning;

use crate::cpu::x86::tasks::TaskRegister;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct TaskReturnSources {
    current: TaskRegister,
    gdt_limit: u16,
}

pub use crate::cpu::x86::tasks::TaskReturnAdmission;
