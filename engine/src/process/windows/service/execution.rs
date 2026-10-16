use super::{WindowsService32, WindowsServiceCleanup32, policy::GuardFailure32};
use crate::{
    memory::{GuestAddress, PageRange},
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::CallFrame32,
};

impl EngineInstance {
    pub(in crate::process) fn execute_pending_guarded_service(
        &mut self,
        owner: PendingOwner,
        token: u32,
        execute: impl FnOnce(&mut Self) -> Result<(), HostError>,
    ) -> Result<(), HostError> {
        if let Some(scope) = &self.windows_service {
            if scope.owner != owner
                || scope.token != token
                || scope.thread_id != self.current_windows_thread_id()?
            {
                return Err(HostError::Call(CallError::Busy));
            }
            return execute(self);
        }
        let pending = self.checked_pending_call(owner, token)?;
        let frame = pending.frame;
        let exit =
            crate::abi::x86::decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let crate::cpu::ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Call(CallError::InvalidStop));
        };
        self.execute_guarded_windows_service(owner, token, id, frame, execute)
    }

    pub(in crate::process::windows) fn execute_guarded_windows_service(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
        execute: impl FnOnce(&mut Self) -> Result<(), HostError>,
    ) -> Result<(), HostError> {
        if self.windows_service.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let thread_id = self.current_windows_thread_id()?;
        self.memory()?
            .begin_guard_service()
            .map_err(HostError::Memory)?;
        self.windows_service = Some(WindowsService32 {
            owner,
            token,
            frame,
            id,
            thread_id,
            returned: None,
            thread: None,
            cleanup: 0,
        });
        let result = execute(self);
        let fault = self.memory.as_ref().unwrap().end_guard_service();
        let mut service = self.windows_service.take().unwrap();
        let Some(fault) = fault else {
            return result;
        };
        if self.current_windows_thread_id()? != service.thread_id
            || !fault.is_current(self.memory()?)
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if service.returned.is_none()
            && result.is_err_and(super::policy::is_memory_fault)
            && !self.windows_provider_owns_token(service.token)
            && self.module_windows_reply.is_none()
            && let Some(failure) = GuardFailure32::for_gate(service.id)
        {
            service.returned = Some(Self::prepare_completed_call(
                service.frame.complete(failure.value),
            )?);
            if let Some(error) = failure.last_error {
                service.thread = Some(self.windows_thread.with_last_error(error));
            }
        }
        if service.returned.is_some() {
            self.checked_pending_call(service.owner, service.token)?;
        }
        if service.cleanup & WindowsServiceCleanup32::Callback as u8 != 0
            && self
                .windows_replies
                .last()
                .is_none_or(|reply| !reply.matches_call(service.owner, service.token))
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if service.cleanup & WindowsServiceCleanup32::Files as u8 != 0 {
            self.check_guard_service_file_reply(service.owner, service.token)?;
        }
        if service.cleanup & WindowsServiceCleanup32::Wait as u8 != 0 {
            self.check_guard_service_wait_reply(service.owner, service.token)?;
        }
        let mapping = self.file_mapping_memory_info(fault.address().0)?;
        let word = service.thread.and_then(|thread| {
            self.module_teb_address()
                .map(|teb| (GuestAddress(teb + 0x34), thread.last_error().to_le_bytes()))
        });
        let writes = word
            .as_ref()
            .map(|(address, bytes)| [(*address, bytes.as_slice())]);
        self.memory
            .as_mut()
            .unwrap()
            .apply_private_pages(
                &[fault.clear()],
                writes.as_ref().map_or(&[], |writes| writes.as_slice()),
            )
            .map_err(HostError::Memory)?;
        let range = PageRange::new(fault.address(), 1).expect("guard page range");
        if let Some(mapping) = mapping {
            self.record_file_mapping_protection(range, mapping.protection & !0x100);
        }
        self.commit_resource_mapping_protection(range);
        if let Some(thread) = service.thread {
            self.windows_thread = thread;
        }
        if service.cleanup & WindowsServiceCleanup32::Callback as u8 != 0 {
            self.windows_replies.pop();
        }
        if service.cleanup & WindowsServiceCleanup32::Files as u8 != 0 {
            self.commit_guard_service_file_reply();
        }
        if service.cleanup & WindowsServiceCleanup32::Wait as u8 != 0 {
            self.commit_guard_service_wait_reply();
        }
        if let Some(prepared) = service.returned {
            self.publish_prepared_call(prepared);
            Ok(())
        } else {
            result
        }
    }
}
