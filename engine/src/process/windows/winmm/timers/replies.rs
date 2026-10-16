use crate::{
    process::{EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, winmm::timers::MultimediaTimerCallback32},
};
#[derive(Clone, Copy)]
pub(in crate::process::windows::winmm) struct TimerReply32 {
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub callback: MultimediaTimerCallback32,
}
#[derive(Default)]
pub(in crate::process) struct WinmmThreadReply32 {
    pub(in crate::process::windows::winmm) reply: Option<TimerReply32>,
}
impl super::super::WinmmRuntime32 {
    pub(in crate::process) fn take_thread_reply(&mut self) -> WinmmThreadReply32 {
        WinmmThreadReply32 {
            reply: self.reply.take(),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: WinmmThreadReply32) {
        assert!(self.reply.is_none());
        self.reply = reply.reply;
    }
    pub(in crate::process) fn empty_thread_reply(&self) -> WinmmThreadReply32 {
        WinmmThreadReply32::default()
    }
    pub(in crate::process) fn thread_reply_ready(&self, reply: &WinmmThreadReply32) -> bool {
        reply
            .reply
            .is_some_and(|reply| !self.timers.callback_pending(reply.callback))
    }
    pub(in crate::process) fn active_thread_reply_ready(&self) -> bool {
        self.reply
            .is_some_and(|reply| !self.timers.callback_pending(reply.callback))
    }
    pub(in crate::process) fn pending_owner(&self) -> Option<PendingOwner> {
        self.reply.map(|reply| reply.owner)
    }
}
impl EngineInstance {
    pub(in crate::process) fn winmm_provider_owns_token(&self, token: u32) -> bool {
        self.winmm.reply.is_some_and(|reply| reply.token == token)
    }
    pub(in crate::process) fn resume_winmm_timer_call(&mut self) -> Result<bool, HostError> {
        let Some(reply) = self.winmm.reply else {
            return Ok(false);
        };
        if self.winmm.timers.callback_pending(reply.callback) {
            return Ok(false);
        }
        self.checked_pending_call(reply.owner, reply.token)?;
        self.validate_windows_provider_owner(reply.owner)?;
        let result = Self::prepare_completed_call(reply.frame.complete(0))?;
        self.winmm.reply = None;
        self.publish_prepared_call(result);
        Ok(true)
    }
}

impl WinmmThreadReply32 {
    pub(in crate::process) fn pending_owner(&self) -> Option<PendingOwner> {
        self.reply.map(|reply| reply.owner)
    }
}
