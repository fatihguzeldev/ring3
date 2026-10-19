use crate::{
    abi::arena::{ARENA_SIZE, CANCEL_OFFSET},
    process::{
        CpuEvent32, EngineInstance, HostError,
        calls::{
            call::{PendingCall, PendingOwner},
            callback::SuspendedCallback,
        },
        cpu::context::ExtendedCpuState,
        windows::{
            activation::ActivationThreadReply32,
            callbacks::WindowsReply32,
            d3d8::{D3D8Reply32, D3D8ThreadReply32},
            dinput::DirectInputThreadReply32,
            dsound::DSoundThreadReply32,
            files::FileThreadReply32,
            modules::ModuleWindowsReply,
            registry::RegistryReply32,
            synchronization::SynchronizationThreadReply32,
            user32::User32ThreadReply32,
            version::VersionThreadReply32,
            winmm::WinmmThreadReply32,
        },
    },
    windows::ThreadState32,
};

pub(super) struct SavedThread32 {
    artifact: Option<crate::cpu::dbt::CompiledRegion>,
    generation: u32,
    pub arena: [u8; ARENA_SIZE],
    pub pending: Option<PendingCall>,
    callback: Option<SuspendedCallback>,
    event: Option<CpuEvent32>,
    windows_thread: ThreadState32,
    windows_replies: Vec<WindowsReply32>,
    module_reply: Option<ModuleWindowsReply>,
    module_state: crate::process::loading::modules::ModuleThreadState32,
    d3d8_reply: Option<D3D8Reply32>,
    d3d8: Option<D3D8ThreadReply32>,
    dsound: Option<DSoundThreadReply32>,
    dinput: Option<DirectInputThreadReply32>,
    files: Option<FileThreadReply32>,
    synchronization: Option<SynchronizationThreadReply32>,
    pub user32: Option<User32ThreadReply32>,
    registry: Option<RegistryReply32>,
    activation: Option<ActivationThreadReply32>,
    version: Option<VersionThreadReply32>,
    winmm: WinmmThreadReply32,
    #[cfg(any(target_arch = "wasm32", test))]
    detection: Option<Box<[crate::process::cpu::events::detection::DetectionSlot32; 1]>>,
}
impl SavedThread32 {
    pub(super) fn empty(engine: &EngineInstance, arena: [u8; ARENA_SIZE]) -> Self {
        Self {
            artifact: None,
            generation: 0,
            arena,
            pending: None,
            callback: None,
            event: None,
            windows_thread: Default::default(),
            windows_replies: Vec::new(),
            module_reply: None,
            module_state: crate::process::loading::modules::ModuleThreadState32::empty(),
            d3d8_reply: None,
            d3d8: engine
                .d3d8
                .as_ref()
                .map(|runtime| runtime.empty_thread_reply()),
            dsound: engine
                .dsound
                .as_ref()
                .map(|runtime| runtime.empty_thread_reply()),
            dinput: engine
                .dinput
                .as_ref()
                .map(|runtime| runtime.empty_thread_reply()),
            files: engine
                .files
                .as_ref()
                .map(|runtime| runtime.empty_thread_reply()),
            synchronization: engine
                .synchronization
                .as_ref()
                .map(|runtime| runtime.empty_thread_reply()),
            user32: engine
                .user32
                .as_ref()
                .map(|runtime| runtime.empty_thread_reply()),
            registry: None,
            activation: None,
            version: None,
            winmm: engine.winmm.empty_thread_reply(),
            #[cfg(any(target_arch = "wasm32", test))]
            detection: None,
        }
    }
    pub(super) fn files_reply(&self) -> Option<&FileThreadReply32> {
        self.files.as_ref()
    }
    pub(super) fn dinput_reply(&self) -> Option<&DirectInputThreadReply32> {
        self.dinput.as_ref()
    }
    pub(super) fn check_owned_scratch(
        &self,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), HostError> {
        if let Some(reply) = self.user32.as_ref() {
            reply
                .check_owned_thread_scratch(memory)
                .map_err(HostError::User32)?;
        }
        if let Some(reply) = self.dinput.as_ref() {
            crate::process::windows::dinput::DirectInputRuntime32::check_thread_scratch(
                reply, memory,
            )?;
        }
        Ok(())
    }
    pub(super) fn scratch_ranges(&self) -> impl Iterator<Item = crate::memory::PageRange> + '_ {
        self.user32
            .iter()
            .flat_map(|reply| reply.scratch_ranges())
            .chain(self.dinput.iter().flat_map(
                crate::process::windows::dinput::DirectInputRuntime32::thread_scratch_ranges,
            ))
    }
    pub(super) fn ready(&self, engine: &EngineInstance, now: u64, thread: u32) -> bool {
        self.version
            .as_ref()
            .is_some_and(|reply| engine.version_thread_reply_ready(reply))
            || self
                .activation
                .as_ref()
                .is_some_and(|reply| engine.activation_thread_reply_ready(reply))
            || engine.registered_com_wait_ready(thread)
            || engine.com_proxy_wait_ready(thread)
            || engine.com_abandoned_work_ready(thread)
            || engine.exceptions.thread_unhandled(thread)
            || engine.winmm.thread_reply_ready(&self.winmm)
            || self
                .dinput
                .as_ref()
                .and_then(|reply| reply.file_request())
                .is_some_and(|request| engine.provider_file_request_ready(request))
            || self
                .files
                .as_ref()
                .is_some_and(|reply| reply.alertable() && engine.has_pending_thread_apc(thread))
            || self.event.is_some()
            || self
                .windows_replies
                .last()
                .is_some_and(WindowsReply32::ready)
            || self
                .files
                .as_ref()
                .zip(engine.files.as_ref())
                .is_some_and(|(reply, runtime)| {
                    runtime.thread_reply_ready(reply, now, |id| engine.threads.is_runnable(id))
                })
            || self
                .d3d8_reply
                .as_ref()
                .zip(engine.d3d8.as_ref())
                .is_some_and(|(reply, runtime)| reply.ready(runtime))
            || self
                .registry
                .as_ref()
                .zip(engine.registry.as_ref())
                .is_some_and(|(reply, runtime)| runtime.thread_reply_ready(reply))
            || self
                .synchronization
                .as_ref()
                .zip(engine.synchronization.as_ref())
                .is_some_and(|(reply, runtime)| reply.ready(runtime.context()))
            || self
                .user32
                .as_ref()
                .is_some_and(|reply| engine.user32_thread_reply_ready(reply))
            || self.module_state.source_ready(engine)
            || self.module_reply.as_ref().is_some_and(|reply| {
                reply.waiting_loader()
                    && engine
                        .loader_initialization_owner()
                        .is_none_or(|owner| owner == thread)
            })
    }
    pub(super) fn deadline(&self) -> Option<u64> {
        self.synchronization
            .as_ref()
            .and_then(|reply| reply.deadline())
            .into_iter()
            .chain(self.files.as_ref().and_then(|reply| reply.deadline()))
            .min()
    }
    pub(super) fn blocks_context_change(&self) -> bool {
        self.pending.is_some() || self.event.is_some()
    }
    pub(super) fn waiting_for_user32(&self) -> bool {
        self.windows_replies
            .last()
            .is_some_and(WindowsReply32::waiting_for_user32)
    }
    fn pins(&self) -> impl Iterator<Item = u64> + '_ {
        self.pending
            .as_ref()
            .and_then(|call| owner_pin(call.owner))
            .into_iter()
            .chain(self.event.as_ref().and_then(|event| owner_pin(event.owner)))
            .chain(self.module_state.execution_pins())
            .chain(self.winmm.pending_owner().and_then(owner_pin))
            .chain(
                self.dinput
                    .as_ref()
                    .and_then(|reply| reply.file_owner())
                    .and_then(owner_pin),
            )
            .chain(
                self.module_reply
                    .as_ref()
                    .and_then(|reply| owner_pin(reply.owner())),
            )
            .chain(
                self.windows_replies
                    .iter()
                    .filter_map(|reply| owner_pin(reply.owner())),
            )
            .chain(
                std::iter::successors(self.callback.as_ref(), |callback| {
                    callback.parent.as_ref().map(|parent| &parent[0])
                })
                .flat_map(|callback| {
                    let mut pins = [None; 4];
                    pins[0] = owner_pin(callback.outer.owner);
                    if let crate::process::calls::callback::SuspendedRecord::Resident {
                        record,
                        active_unit_id,
                        ..
                    } = callback.record
                    {
                        pins[1] = Some(record.outer_unit_id);
                        pins[2] = Some(record.callback_unit_id);
                        pins[3] = Some(active_unit_id);
                    }
                    pins.into_iter().flatten()
                }),
            )
    }
}
fn owner_pin(owner: PendingOwner) -> Option<u64> {
    match owner {
        PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => Some(id),
        PendingOwner::Replacement(_) => None,
    }
}
impl super::ThreadRuntime32 {
    pub(in crate::process) fn windows_callback_result(
        &self,
        thread: u32,
        provider: crate::process::windows::callbacks::WindowsCallbackProvider32,
        owner: PendingOwner,
        token: u32,
        continuation: u64,
        frame: crate::windows::CallFrame32,
    ) -> Option<u32> {
        let record = self.threads.iter().find(|record| record.id == thread)?;
        record
            .saved
            .iter()
            .chain(
                record
                    .active_invocations
                    .iter()
                    .map(|invocation| &invocation.saved),
            )
            .flat_map(|saved| &saved.windows_replies)
            .find_map(|reply| reply.received_result(provider, owner, token, continuation, frame))
    }

    pub(in crate::process) fn execution_pins(&self) -> impl Iterator<Item = u64> + '_ {
        self.threads
            .iter()
            .filter_map(|thread| thread.saved.as_ref())
            .flat_map(SavedThread32::pins)
            .chain(
                self.threads
                    .iter()
                    .flat_map(|thread| &thread.active_invocations)
                    .flat_map(|invocation| invocation.saved.pins()),
            )
    }
}

#[cfg(test)]
mod tests;
impl EngineInstance {
    pub(super) fn take_windows_thread_context(&mut self) -> Result<SavedThread32, HostError> {
        ExtendedCpuState::capture(self.arena())?;
        self.revoke_owned_execution();
        let saved = SavedThread32 {
            artifact: self.artifact.take(),
            generation: std::mem::take(&mut self.generation),
            arena: self.arena().try_into().unwrap(),
            pending: self.pending_call.take(),
            callback: self.callback.take(),
            event: self.cpu_event.take(),
            windows_thread: self.windows_thread,
            windows_replies: std::mem::take(&mut self.windows_replies),
            module_reply: self.module_windows_reply.take(),
            module_state: self.take_module_thread_state(),
            winmm: self.winmm.take_thread_reply(),
            version: self.version.take_thread_reply(),
            d3d8_reply: self.d3d8_reply.take(),
            d3d8: self
                .d3d8
                .as_mut()
                .map(|runtime| runtime.take_thread_reply()),
            dsound: self
                .dsound
                .as_mut()
                .map(|runtime| runtime.take_thread_reply()),
            dinput: self
                .dinput
                .as_mut()
                .map(|runtime| runtime.take_thread_reply()),
            files: self
                .files
                .as_mut()
                .map(|runtime| runtime.take_thread_reply()),
            synchronization: self
                .synchronization
                .as_mut()
                .map(|runtime| runtime.take_thread_reply()),
            user32: self
                .user32
                .as_mut()
                .map(|runtime| runtime.take_thread_reply()),
            registry: self
                .registry
                .as_mut()
                .and_then(|runtime| runtime.take_thread_reply(self.threads.current_id())),
            activation: self
                .activation
                .as_mut()
                .and_then(|runtime| runtime.take_thread_reply()),
            #[cfg(any(target_arch = "wasm32", test))]
            detection: self.exception_detection.take(),
        };
        #[cfg(any(target_arch = "wasm32", test))]
        self.refresh_cpu_read32_view();
        Ok(saved)
    }
    pub(super) fn restore_windows_thread_context(&mut self, saved: SavedThread32) {
        assert!(
            self.pending_call.is_none()
                && self.callback.is_none()
                && self.windows_replies.is_empty()
        );
        self.revoke_owned_execution();
        let cancel: [u8; 4] = self.arena()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
            .try_into()
            .unwrap();
        self.arena.as_mut().get_mut().copy_from_slice(&saved.arena);
        self.arena.as_mut().get_mut()[CANCEL_OFFSET..CANCEL_OFFSET + 4].copy_from_slice(&cancel);
        assert!(self.artifact.is_none());
        self.artifact = saved.artifact;
        self.generation = saved.generation;
        self.pending_call = saved.pending;
        self.callback = saved.callback;
        self.cpu_event = saved.event;
        self.windows_thread = saved.windows_thread;
        self.windows_replies = saved.windows_replies;
        self.module_windows_reply = saved.module_reply;
        self.restore_module_thread_state(saved.module_state);
        self.winmm.restore_thread_reply(saved.winmm);
        self.version.restore_thread_reply(saved.version);
        self.d3d8_reply = saved.d3d8_reply;
        if let (Some(runtime), Some(reply)) = (self.d3d8.as_mut(), saved.d3d8) {
            runtime.restore_thread_reply(reply);
        }
        if let (Some(runtime), Some(reply)) = (self.dsound.as_mut(), saved.dsound) {
            runtime.restore_thread_reply(reply);
        }
        if let (Some(runtime), Some(reply)) = (self.dinput.as_mut(), saved.dinput) {
            runtime.restore_thread_reply(reply);
        }
        if let (Some(runtime), Some(reply)) = (self.files.as_mut(), saved.files) {
            runtime.restore_thread_reply(reply);
        }
        if let (Some(runtime), Some(reply)) = (self.synchronization.as_mut(), saved.synchronization)
        {
            runtime.restore_thread_reply(reply);
        }
        if let (Some(runtime), Some(reply)) = (self.user32.as_mut(), saved.user32) {
            runtime.restore_thread_reply(reply);
        }
        if let Some(runtime) = self.registry.as_mut() {
            runtime
                .restore_thread_reply(saved.registry)
                .expect("active registry reply was parked before thread restore");
        }
        if let Some(runtime) = self.activation.as_mut() {
            runtime.restore_thread_reply(saved.activation);
        }
        #[cfg(any(target_arch = "wasm32", test))]
        {
            self.exception_detection = saved.detection;
            self.refresh_cpu_read32_view();
        }
    }
}

impl super::ThreadRecord32 {
    pub(super) fn check_remote_scratch(
        &self,
        continuation: u64,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), crate::windows::user32::User32Error32> {
        let reply = self
            .saved
            .iter()
            .chain(
                self.active_invocations
                    .iter()
                    .map(|invocation| &invocation.saved),
            )
            .filter_map(|saved| saved.user32.as_ref())
            .find(|reply| reply.check_remote_result(continuation).is_ok())
            .ok_or(crate::windows::user32::User32Error32::InvalidToken)?;
        reply.check_remote_scratch(continuation, memory)
    }
    pub(super) fn check_remote_result(
        &self,
        continuation: u64,
    ) -> Result<(), crate::windows::user32::User32Error32> {
        self.saved
            .iter()
            .chain(
                self.active_invocations
                    .iter()
                    .map(|invocation| &invocation.saved),
            )
            .filter_map(|saved| saved.user32.as_ref())
            .find(|reply| reply.check_remote_result(continuation).is_ok())
            .ok_or(crate::windows::user32::User32Error32::InvalidToken)?;
        Ok(())
    }
    pub(super) fn accept_remote_result(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<(), crate::windows::user32::User32Error32> {
        let reply = self
            .saved
            .iter_mut()
            .chain(
                self.active_invocations
                    .iter_mut()
                    .map(|invocation| &mut invocation.saved),
            )
            .filter_map(|saved| saved.user32.as_mut())
            .find(|reply| reply.check_remote_result(continuation).is_ok())
            .ok_or(crate::windows::user32::User32Error32::InvalidToken)?;
        reply.accept_remote_result(continuation, result)
    }
}

impl super::ThreadRuntime32 {
    pub(super) fn check_user32_remote_scratch(
        &self,
        sender: u32,
        continuation: u64,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), crate::windows::user32::User32Error32> {
        let sender = self
            .threads
            .iter()
            .find(|thread| thread.id == sender && thread.state != super::ThreadState32::Terminated)
            .ok_or(crate::windows::user32::User32Error32::InvalidToken)?;
        sender.check_remote_scratch(continuation, memory)
    }
}
