use crate::{
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        shell::{ShellApi32 as Api, ShellError32 as E},
    },
};
impl EngineInstance {
    pub fn register_shell32_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::shell::shell32_module(base))
    }

    pub(in crate::process) fn try_complete_shell_call(
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
        self.checked_pending_call(owner, token)?;
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let result = if api == Api::CommandLineToArgvW {
            self.shell_arguments(frame.arguments())
        } else {
            self.shell_folder(api, frame.arguments())
        };
        let value = match result {
            Ok(value) => value,
            Err(HostError::Memory(error) | HostError::Shell(E::Memory(error))) => {
                return Err(HostError::Memory(error));
            }
            Err(error) => {
                let code = match error {
                    HostError::Shell(error) => error.win32(),
                    HostError::File(error) => error.win32(),
                    HostError::WindowsMemory(error) => error.win32(),
                    other => return Err(other),
                };
                if matches!(api, Api::SHGetFolderPathA | Api::SHGetFolderPathW) {
                    let pointer = frame.arguments()[4];
                    if pointer != 0 {
                        self.memory
                            .as_mut()
                            .unwrap()
                            .write(
                                GuestAddress(pointer),
                                if api == Api::SHGetFolderPathW {
                                    &[0, 0]
                                } else {
                                    &[0]
                                },
                            )
                            .map_err(HostError::Memory)?;
                    }
                    E::Win32(code).hresult()
                } else {
                    let thread = self.current_windows_thread()?.with_last_error(code);
                    self.publish_windows_thread(thread)?;
                    0
                }
            }
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
}
impl From<super::super::strings::WindowsStringError32> for E {
    fn from(error: super::super::strings::WindowsStringError32) -> Self {
        use super::super::strings::WindowsStringError32 as S;
        match error {
            S::InvalidParameter => Self::InvalidParameter,
            S::FilenameTooLong => Self::Win32(206),
            S::Allocation => Self::Allocation,
            S::Memory(error) => Self::Memory(error),
        }
    }
}
