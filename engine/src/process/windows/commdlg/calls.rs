use super::{CapturedFileDialog32, publication::encode};
use crate::{
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{strings::Input32, user32::User32Outcome32},
    },
    windows::{
        CallFrame32,
        commdlg::{
            ComdlgApi32 as Api, ComdlgError32 as E, DIALOG_PROCEDURE_A, DIALOG_PROCEDURE_W,
            file_title,
        },
    },
};

impl EngineInstance {
    pub(in crate::process) fn try_complete_commdlg_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let api = Api::from_id(id);
        if api.is_none() && !matches!(id, DIALOG_PROCEDURE_A | DIALOG_PROCEDURE_W) {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        let pending = self.checked_pending_call(owner, token)?;
        let count = api.map_or(4, Api::stack_words);
        if pending.frame != frame
            || frame.stack_words() != count
            || frame.convention() != crate::windows::CallingConvention32::Stdcall
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let thread = self.current_windows_thread_id()?;
        if api == Some(Api::CommDlgExtendedError) {
            let value = self.comdlg.error(thread);
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
            return Ok(true);
        }
        if let Some(api @ (Api::GetFileTitleA | Api::GetFileTitleW)) = api {
            let value = self.common_file_title(api, frame.arguments())?;
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
            return Ok(true);
        }
        self.validate_new_windows_reply(owner, token)?;
        self.reserve_windows_reply()?;
        let mut last_error = 0;
        let outcome = if let Some(api) = api {
            let prepared = self
                .comdlg
                .prepare_error(thread, self.threads.thread_limit())
                .map_err(HostError::Comdlg)?;
            let ansi = self.windows_ansi_code_page();
            let request = CapturedFileDialog32::read(
                self.memory.as_mut().ok_or(HostError::Closed)?,
                frame.arguments()[0],
                api.wide(),
                ansi,
                1 << 20,
            );
            match request {
                Ok(request) => match self.begin_common_file_dialog(api, request, owner, token) {
                    Ok(outcome) => {
                        self.comdlg.commit_error(prepared, 0);
                        outcome
                    }
                    Err(HostError::User32(crate::windows::user32::User32Error32::Win32(code))) => {
                        self.comdlg.commit_error(prepared, 2);
                        last_error = code;
                        User32Outcome32::Return(0)
                    }
                    Err(HostError::File(error))
                        if !matches!(
                            error,
                            crate::files::FileError32::Allocation
                                | crate::files::FileError32::Capacity
                                | crate::files::FileError32::Exhausted
                        ) =>
                    {
                        self.comdlg.commit_error(prepared, 2);
                        last_error = error.win32();
                        User32Outcome32::Return(0)
                    }
                    Err(HostError::Comdlg(E::Dialog(error))) => {
                        self.comdlg.commit_error(prepared, error as u32);
                        last_error = 87;
                        User32Outcome32::Return(0)
                    }
                    Err(error) => return Err(error),
                },
                Err(E::Dialog(error)) => {
                    self.comdlg.commit_error(prepared, error as u32);
                    last_error = 87;
                    User32Outcome32::Return(0)
                }
                Err(error) => return Err(HostError::Comdlg(error)),
            }
        } else {
            self.execute_common_file_dialog_procedure(frame.arguments(), id == DIALOG_PROCEDURE_W)?
        };
        self.publish_user32_result(owner, token, frame, outcome, Some(last_error))?;
        Ok(true)
    }
    pub(in crate::process::windows) fn set_common_dialog_error(
        &mut self,
        error: u32,
    ) -> Result<(), HostError> {
        let thread = self.current_windows_thread_id()?;
        let prepared = self
            .comdlg
            .prepare_error(thread, self.threads.thread_limit())
            .map_err(HostError::Comdlg)?;
        self.comdlg.commit_error(prepared, error);
        Ok(())
    }
    fn common_file_title(&mut self, api: Api, args: &[u32]) -> Result<u32, HostError> {
        let input = Input32::read(
            self.memory.as_ref().ok_or(HostError::Closed)?,
            args[0],
            api.wide(),
            self.windows_ansi_code_page(),
            32768,
        )
        .map_err(|error| match error {
            crate::process::windows::strings::WindowsStringError32::Memory(error) => {
                HostError::Memory(error)
            }
            _ => HostError::Comdlg(E::InvalidRequest),
        })?;
        let Some(title) = file_title(&input.units) else {
            return Ok(u32::MAX);
        };
        let bytes =
            encode(title, api.wide(), self.windows_ansi_code_page()).map_err(HostError::Comdlg)?;
        let needed = bytes.len() / if api.wide() { 2 } else { 1 };
        if (args[2] as usize) < needed {
            return u32::try_from(needed).map_err(|_| HostError::Comdlg(E::Capacity));
        }
        self.memory
            .as_mut()
            .ok_or(HostError::Closed)?
            .write(GuestAddress(args[1]), &bytes)
            .map_err(HostError::Memory)?;
        Ok(0)
    }
}
