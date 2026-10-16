use crate::{
    files::FileError32,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, security::SecurityApi32 as Api},
};

impl EngineInstance {
    pub(in crate::process) fn try_complete_security_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let args = frame.arguments();
        if api.returns_status() {
            let value = self.execute_security_information(
                !matches!(api, Api::GetSecurityInfo | Api::SetSecurityInfo),
                matches!(api, Api::GetNamedSecurityInfoW | Api::SetNamedSecurityInfoW),
                matches!(
                    api,
                    Api::GetSecurityInfo | Api::GetNamedSecurityInfoA | Api::GetNamedSecurityInfoW
                ),
                args,
            )?;
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
            return Ok(true);
        }
        let outcome = if matches!(
            api,
            Api::IsValidSecurityDescriptor | Api::GetSecurityDescriptorLength
        ) {
            super::memory::descriptor_and_length(self.memory()?, args[0]).map(|(_, length)| {
                (
                    if api == Api::IsValidSecurityDescriptor {
                        1
                    } else {
                        length
                    },
                    None,
                )
            })
        } else {
            if matches!(api, Api::GetFileSecurityA | Api::GetFileSecurityW) {
                self.memory()?
                    .ensure_transaction_versions(2)
                    .map_err(HostError::Memory)?;
            }
            let ansi = self.windows_ansi_code_page();
            if let Some(files) = self.files.as_mut() {
                files.set_security_ansi(ansi);
                files.execute_file_security(
                    self.memory.as_mut().unwrap(),
                    api,
                    args,
                    self.windows_security.token(),
                )
            } else {
                Err(FileError32::Unsupported)
            }
        };
        let (value, error) = outcome.unwrap_or_else(|error| (0, Some(error.win32())));
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        if let Some(error) = error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(prepared);
        Ok(true)
    }
}
