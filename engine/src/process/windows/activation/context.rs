use super::{sources::CreateWork32, storage::ContextStorage32};
use crate::{
    files::FileReadRequest32,
    memory::PageRange,
    process::calls::call::PendingOwner,
    windows::{
        CallFrame32,
        activation::{ActivationConfig32, ActivationContexts32, ActivationError32 as E},
    },
};

pub(in crate::process) struct ActivationRuntime32 {
    pub(super) config: ActivationConfig32,
    pub(super) configured: bool,
    pub(super) native: ActivationContexts32,
    pub(super) storage: Vec<ContextStorage32>,
    pub(super) used: usize,
    pub(super) reply: Option<ActivationThreadReply32>,
    pub(super) module_contexts: Vec<(crate::loader::ModuleId32, u32)>,
    pub(super) captures: Vec<super::captures::ManagedActivationCapture32>,
}
pub(in crate::process) struct ActivationThreadReply32 {
    pub(super) owner: PendingOwner,
    pub(super) token: u32,
    pub(super) frame: CallFrame32,
    pub(super) thread: u32,
    pub(super) api: crate::windows::activation::ActivationApi32,
    pub(super) request: Option<FileReadRequest32>,
    pub(super) work: CreateWork32,
}
impl ActivationRuntime32 {
    pub(in crate::process) fn new(config: ActivationConfig32) -> Result<Self, E> {
        let native = ActivationContexts32::new(config)?;
        let mut storage = Vec::new();
        storage
            .try_reserve_exact(config.contexts as usize)
            .map_err(|_| E::NotEnoughMemory)?;
        Ok(Self {
            config,
            configured: false,
            native,
            storage,
            used: 0,
            reply: None,
            module_contexts: Vec::new(),
            captures: Vec::new(),
        })
    }
    pub(in crate::process) fn reservation(&self) -> PageRange {
        self.config.section_bounds
    }
    pub(in crate::process) fn pending_owner(&self) -> Option<PendingOwner> {
        self.reply.as_ref().map(|reply| reply.owner)
    }
    pub(in crate::process) fn provider_owns_token(&self, token: u32) -> bool {
        self.reply
            .as_ref()
            .is_some_and(|reply| reply.token == token)
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> Option<ActivationThreadReply32> {
        self.reply.take()
    }
    pub(in crate::process) fn restore_thread_reply(
        &mut self,
        reply: Option<ActivationThreadReply32>,
    ) {
        assert!(
            self.reply.is_none(),
            "activation reply was parked before thread restore"
        );
        self.reply = reply;
    }
    pub(super) fn storage(&self, handle: u32) -> Result<&ContextStorage32, E> {
        self.storage
            .iter()
            .find(|storage| storage.handle == handle)
            .ok_or(E::InvalidHandle)
    }
    pub(super) fn retire_storage(&mut self, handles: &[u32]) {
        self.storage.retain(|storage| {
            if handles.contains(&storage.handle) {
                self.used -= storage.cost;
                false
            } else {
                true
            }
        });
    }
}
