mod context;
mod execution;
mod replies;

use crate::{
    memory::GuestAddress,
    process::{HostError, calls::call::PendingOwner},
    windows::CallFrame32,
};

pub const WINDOWS_CALLBACK_ARGUMENTS32: usize = 16;
const MAX_REPLIES: usize = 33;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsCallbackProvider32 {
    User32,
    D3D8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowsCallback32 {
    pub procedure: GuestAddress,
    pub arguments: [u32; WINDOWS_CALLBACK_ARGUMENTS32],
    pub count: u32,
    pub continuation: u64,
    pub provider: WindowsCallbackProvider32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsProviderOutcome32 {
    Return(u32),
    Callback(WindowsCallback32),
}

#[derive(Clone, Copy)]
pub(in crate::process) struct WindowsReply32 {
    owner: PendingOwner,
    token: u32,
    frame: CallFrame32,
    provider: WindowsCallbackProvider32,
    continuation: u64,
    last_error: Option<u32>,
    stage: ReplyStage32,
}

#[derive(Clone, Copy)]
enum ReplyStage32 {
    Ready(WindowsCallback32),
    Resident { callback_id: u64, token: u32 },
    Replacement { generation: u32, token: u32 },
    Received(u32),
    Complete(u32),
    Wait,
}

fn validate_request(request: WindowsCallback32) -> Result<(), HostError> {
    if request.procedure.0 == 0
        || request.continuation == 0
        || request.count as usize > WINDOWS_CALLBACK_ARGUMENTS32
    {
        return Err(HostError::InvalidRequest);
    }
    Ok(())
}
