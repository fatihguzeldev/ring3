use crate::{
    process::calls::call::{PendingOwner, PreparedCall32},
    windows::{CallFrame32, ThreadState32},
};

pub(in crate::process) struct WindowsService32 {
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) frame: CallFrame32,
    pub(super) id: u32,
    pub(super) thread_id: u32,
    pub(super) returned: Option<PreparedCall32>,
    pub(super) thread: Option<ThreadState32>,
    pub(super) cleanup: u8,
}

#[derive(Clone, Copy)]
pub(in crate::process) enum WindowsServiceCleanup32 {
    Callback = 1,
    Files = 2,
    Wait = 4,
}
