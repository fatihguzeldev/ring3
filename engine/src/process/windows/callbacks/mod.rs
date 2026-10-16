mod scratch;
pub(in crate::process::windows) use scratch::GuestCallbackScratch32;
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
    DirectSound,
    DirectInput,
    Files,
    Resources,
    Nls,
    Ole32,
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
    activation: Option<crate::windows::activation::ActivationScope32>,
}
impl WindowsReply32 {
    pub(in crate::process) fn provider(&self) -> WindowsCallbackProvider32 {
        self.provider
    }
    pub(in crate::process) fn matches_call(&self, owner: PendingOwner, token: u32) -> bool {
        self.owner == owner && self.token == token
    }

    pub(in crate::process) fn received_result(
        &self,
        provider: WindowsCallbackProvider32,
        owner: PendingOwner,
        token: u32,
        continuation: u64,
        frame: CallFrame32,
    ) -> Option<u32> {
        if self.provider == provider
            && self.matches_call(owner, token)
            && self.continuation == continuation
            && self.frame == frame
            && let ReplyStage32::Received(result) = self.stage
        {
            Some(result)
        } else {
            None
        }
    }
    pub(in crate::process) fn owner(&self) -> PendingOwner {
        self.owner
    }
    pub(in crate::process) fn ready(&self) -> bool {
        matches!(
            self.stage,
            ReplyStage32::Ready(_) | ReplyStage32::Received(_) | ReplyStage32::Complete(_)
        )
    }
    pub(in crate::process) fn waiting_for_user32(&self) -> bool {
        self.provider == WindowsCallbackProvider32::User32
            && matches!(self.stage, ReplyStage32::Wait)
    }
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

#[cfg(test)]
#[path = "tests/results.rs"]
mod result_tests;
