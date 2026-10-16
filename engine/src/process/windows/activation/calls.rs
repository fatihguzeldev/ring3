use super::{
    ActivationRuntime32, ActivationThreadReply32, memory,
    sources::{CreateSource32, CreateWork32},
};
use crate::{
    files::FileIo32,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        activation::{
            ActivationApi32 as Api, ActivationConfig32, ActivationError32 as E,
            ActivationSectionKey32,
        },
        exceptions::ExceptionRecord32,
    },
};

impl EngineInstance {
    pub fn configure_activation(&mut self, config: ActivationConfig32) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some()
            || self.callback.is_some()
            || self.activation.as_ref().is_some_and(|runtime| {
                runtime.configured || !runtime.native.is_empty() || runtime.reply.is_some()
            })
        {
            return Err(HostError::Activation(E::Busy));
        }
        let previous = self
            .activation
            .as_ref()
            .map(ActivationRuntime32::reservation);
        if self.guest_address_reservations()?.into_iter().any(|range| {
            Some(range) != previous
                && crate::process::memory::reservations::overlaps(range, config.section_bounds)
        }) {
            return Err(HostError::InvalidRequest);
        }
        let mut runtime = ActivationRuntime32::new(config).map_err(HostError::Activation)?;
        runtime.configured = true;
        self.activation = Some(runtime);
        Ok(())
    }
    pub(in crate::process) fn try_complete_activation_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.collect_activation_captures()?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        if self.activation.is_none() {
            let config = ActivationConfig32::default();
            self.check_guest_reservation_admission(&[config.section_bounds])?;
            self.activation =
                Some(ActivationRuntime32::new(config).map_err(HostError::Activation)?);
        }
        let thread = self.current_windows_thread_id()?;
        if matches!(api, Api::CreateActCtxA | Api::CreateActCtxW) {
            return self.complete_activation_create(owner, token, thread, api, frame);
        }
        let result = self.activation_call(thread, api, frame.arguments());
        if let Err(error) = result
            && let Some(code) = error.exception()
        {
            let mut original = self.windows_context_state()?;
            original.state = frame.complete(0);
            let record = ExceptionRecord32::new(code, 1, original.state.eip, &[])
                .map_err(HostError::Exception)?;
            self.begin_exception_delivery(record, original, owner, None, None, None)?;
            return Ok(true);
        }
        self.publish_activation_result(api, frame, result)?;
        Ok(true)
    }
    fn complete_activation_create(
        &mut self,
        owner: PendingOwner,
        token: u32,
        thread: u32,
        api: Api,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let existing = self.activation.as_mut().unwrap().reply.take();
        let mut reply = if let Some(reply) = existing {
            if reply.owner != owner
                || reply.api != api
                || reply.token != token
                || reply.thread != thread
                || reply.frame != frame
            {
                self.activation.as_mut().unwrap().reply = Some(reply);
                return Err(HostError::Call(CallError::Busy));
            }
            reply
        } else {
            let source = match CreateSource32::read(
                self.memory()?,
                frame.arguments()[0],
                api == Api::CreateActCtxW,
                self.windows_ansi_code_page(),
                self.current_thread_ui_language(),
            ) {
                Ok(source) => source,
                Err(error) => {
                    self.publish_activation_result(api, frame, Err(error))?;
                    return Ok(true);
                }
            };
            ActivationThreadReply32 {
                owner,
                token,
                frame,
                thread,
                api,
                request: None,
                work: CreateWork32::new(source),
            }
        };
        reply.request = None;
        let limits = self.activation.as_ref().unwrap().config;
        let result = match reply.work.poll(self, limits) {
            Ok(FileIo32::NeedBytes(request)) => {
                reply.request = Some(request);
                self.activation.as_mut().unwrap().reply = Some(reply);
                return Ok(true);
            }
            Ok(FileIo32::Ready(())) => self.publish_activation_context(&reply.work),
            Err(error) => Err(error),
        };
        match result {
            Ok(handle) => self.publish_activation_result(api, frame, Ok(handle))?,
            Err(error) => {
                reply.work.error = Some(error);
                if let Err(error) = self.check_close_provider_files(&reply.work.leases) {
                    self.activation.as_mut().unwrap().reply = Some(reply);
                    return Err(error);
                }
                self.commit_close_provider_files(&reply.work.leases);
                let error = match error {
                    HostError::Activation(error) => error,
                    HostError::File(error) => E::Win32(error.win32()),
                    HostError::Resource(error) => E::Win32(error.win32()),
                    other => return Err(other),
                };
                self.publish_activation_result(api, frame, Err(error))?;
            }
        }
        Ok(true)
    }
    fn publish_activation_result(
        &mut self,
        api: Api,
        frame: CallFrame32,
        result: Result<u32, E>,
    ) -> Result<(), HostError> {
        let (value, error) = match result {
            Ok(value) => (value, None),
            Err(error) => (api.failure_value(), Some(error.win32())),
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        if let Some(error) = error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(prepared);
        Ok(())
    }
    pub(super) fn activation_call(
        &mut self,
        thread: u32,
        api: Api,
        args: &[u32],
    ) -> Result<u32, E> {
        let cp = self.windows_ansi_code_page();
        let module_handle = if api == Api::QueryActCtxW && args[0] & 0x18 != 0 {
            Some(self.query_activation_module_context(args[0], args[1])?)
        } else {
            None
        };
        let runtime = self.activation.as_mut().ok_or(E::InvalidParameter)?;
        let memory = self.memory.as_mut().ok_or(E::NoAccess)?;
        match api {
            Api::AddRefActCtx => {
                runtime.native.add_ref(args[0])?;
                Ok(0)
            }
            Api::ReleaseActCtx => runtime.release(memory, args[0]),
            Api::ActivateActCtx => runtime.activate(memory, thread, args[0], args[1]),
            Api::DeactivateActCtx => runtime.deactivate(memory, thread, args[0], args[1]),
            Api::GetCurrentActCtx => runtime.current(memory, thread, args[0]),
            Api::ZombifyActCtx => {
                runtime.native.zombify(args[0])?;
                Ok(1)
            }
            Api::FindActCtxSectionStringA | Api::FindActCtxSectionStringW => {
                let key =
                    memory::string(memory, args[3], api == Api::FindActCtxSectionStringW, cp)?;
                runtime.find_section(memory, thread, args, ActivationSectionKey32::String(key))
            }
            Api::FindActCtxSectionGuid => {
                let key = memory::read(memory, args[3])?;
                runtime.find_section(memory, thread, args, ActivationSectionKey32::Guid(key))
            }
            Api::QueryActCtxW => runtime.query(memory, thread, args, module_handle),
            Api::QueryActCtxSettingsW => runtime.settings(memory, thread, args),
            Api::CreateActCtxA | Api::CreateActCtxW => unreachable!(),
        }
    }
}
