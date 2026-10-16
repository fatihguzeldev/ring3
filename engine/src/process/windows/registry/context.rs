use crate::{
    process::calls::call::PendingOwner,
    windows::{
        CallFrame32,
        registry::{Registry32, RegistryConfig32, RegistryError32 as E},
    },
};
pub(in crate::process) struct RegistryRuntime32 {
    pub(super) native: Registry32,
    pub(super) reply: Option<RegistryReply32>,
    pub(super) watches: Vec<Option<super::notifications::Registration32>>,
    pub(super) next_watch: u64,
    pub(super) input: Option<super::host_input::RegistrySeedInput32>,
    pub(super) next_input: u64,
}
#[derive(Clone, Copy)]
pub(super) enum RegistryWaitKind32 {
    Flush,
    Notify(u64),
}
#[derive(Clone, Copy)]
pub(in crate::process) struct RegistryReply32 {
    pub(in crate::process) owner: PendingOwner,
    pub(in crate::process) token: u32,
    pub(in crate::process) frame: CallFrame32,
    pub(in crate::process) thread: u32,
    pub(super) wait: RegistryWaitKind32,
}
impl RegistryRuntime32 {
    pub(in crate::process) fn new(epoch: u64, config: RegistryConfig32) -> Result<Self, E> {
        let native = Registry32::new(epoch, config)?;
        let mut watches = Vec::new();
        watches
            .try_reserve_exact(config.limits.handles as usize)
            .map_err(|_| E::NotEnoughMemory)?;
        watches.resize_with(config.limits.handles as usize, || None);
        Ok(Self {
            native,
            reply: None,
            watches,
            next_watch: 0,
            input: None,
            next_input: 1,
        })
    }
    pub(in crate::process) fn pending_owner(&self) -> Option<PendingOwner> {
        self.reply.map(|reply| reply.owner)
    }
    pub(in crate::process) fn take_thread_reply(&mut self, thread: u32) -> Option<RegistryReply32> {
        if self.reply.is_some_and(|reply| reply.thread == thread) {
            self.reply.take()
        } else {
            None
        }
    }
    pub(in crate::process) fn restore_thread_reply(
        &mut self,
        reply: Option<RegistryReply32>,
    ) -> Result<(), E> {
        if self.reply.is_some() && reply.is_some() {
            return Err(E::Busy);
        }
        if reply.is_some() {
            self.reply = reply;
        }
        Ok(())
    }
}
