use super::{VersionThreadReply32, input::VersionInput32};
use crate::{
    files::FileIo32,
    memory::{Access, GuestAddress},
    process::{
        CallError, EngineInstance, HostError, calls::call::PendingOwner, windows::strings::Input32,
    },
    windows::{
        CallFrame32,
        version::{VersionApi32 as Api, VersionBlock32, VersionError32 as E},
    },
};

impl EngineInstance {
    pub fn register_version_module(&mut self, base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::version::version_module(base))
    }

    pub(in crate::process) fn try_complete_version_call(
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
        if matches!(api, Api::VerQueryValueA | Api::VerQueryValueW) {
            let result = self.version_query(api, frame.arguments());
            self.publish_version_result(frame, result)?;
            return Ok(true);
        }
        let thread = self.current_windows_thread_id()?;
        let mut reply = if let Some(reply) = self.version.reply.take() {
            if reply.owner != owner
                || reply.token != token
                || reply.thread != thread
                || reply.api != api
                || reply.frame != frame
            {
                self.version.reply = Some(reply);
                return Err(HostError::Call(CallError::Busy));
            }
            reply
        } else {
            let args = frame.arguments();
            let size_only = matches!(
                api,
                Api::GetFileVersionInfoSizeA | Api::GetFileVersionInfoSizeW
            );
            let output = if size_only {
                if args[1] == 0 {
                    None
                } else {
                    Some((args[1], 4))
                }
            } else {
                if args[2] == 0 || args[3] == 0 {
                    self.publish_version_result(frame, Err(E::InvalidParameter))?;
                    return Ok(true);
                }
                Some((args[3], args[2] as usize))
            };
            if let Some((address, length)) = output {
                self.memory()?
                    .check_span(GuestAddress(address), length, Access::Write)
                    .map_err(HostError::Memory)?;
            }
            let wide = api.name().ends_with('W');
            let name = match Input32::read(
                self.memory()?,
                args[0],
                wide,
                self.windows_ansi_code_page(),
                32768,
            ) {
                Ok(input) => input.units,
                Err(e) => {
                    self.publish_version_result(frame, Err(E::from(e)))?;
                    return Ok(true);
                }
            };
            let name = match String::from_utf16(&name) {
                Ok(name) => name,
                Err(_) => {
                    self.publish_version_result(frame, Err(E::InvalidParameter))?;
                    return Ok(true);
                }
            };
            let path = match self.resolve_module_source_name(&name) {
                Ok(path) => path.encode_utf16().collect::<Vec<_>>(),
                Err(error) => {
                    self.publish_version_result(frame, Err(E::Win32(version_error(&error))))?;
                    return Ok(true);
                }
            };
            let lease = match self.open_provider_file(&path, false) {
                Ok(lease) => lease,
                Err(error) => {
                    self.publish_version_result(frame, Err(E::Win32(version_error(&error))))?;
                    return Ok(true);
                }
            };
            let length = self
                .provider_file_size(lease)
                .expect("new version source lease is current under the same coordinator");
            VersionThreadReply32 {
                owner,
                token,
                frame,
                api,
                thread,
                request: None,
                input: VersionInput32::new(lease, length, self.current_thread_ui_language()),
                data: None,
                error: None,
            }
        };
        reply.request = None;
        if reply.data.is_none() && reply.error.is_none() {
            match reply.input.poll(self) {
                Ok(FileIo32::NeedBytes(request)) => {
                    reply.request = Some(request);
                    self.version.reply = Some(reply);
                    return Ok(true);
                }
                Ok(FileIo32::Ready(bytes)) => match VersionBlock32::parse(&bytes)
                    .and_then(|block| block.encoded(self.windows_ansi_code_page()))
                {
                    Ok(bytes) => reply.data = Some(bytes),
                    Err(e) => reply.error = Some(e.win32()),
                },
                Err(error) => reply.error = Some(version_error(&error)),
            }
        }
        if let Err(error) = self.check_close_provider_files(&[reply.input.lease]) {
            self.version.reply = Some(reply);
            return Err(error);
        }
        let result = if let Some(error) = reply.error {
            Err(E::Win32(error))
        } else {
            let bytes = reply.data.as_ref().unwrap();
            let args = frame.arguments();
            if matches!(
                api,
                Api::GetFileVersionInfoSizeA | Api::GetFileVersionInfoSizeW
            ) {
                if args[1] != 0 {
                    if let Err(error) = self
                        .memory
                        .as_mut()
                        .unwrap()
                        .write(GuestAddress(args[1]), &[0; 4])
                    {
                        self.version.reply = Some(reply);
                        return Err(HostError::Memory(error));
                    }
                }
                Ok(bytes.len() as u32)
            } else {
                let length = (args[2] as usize).min(bytes.len());
                if let Err(error) = self
                    .memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[3]), &bytes[..length])
                {
                    self.version.reply = Some(reply);
                    return Err(HostError::Memory(error));
                }
                Ok(1)
            }
        };
        self.commit_close_provider_files(&[reply.input.lease]);
        self.publish_version_result(frame, result)?;
        Ok(true)
    }
    fn publish_version_result(
        &mut self,
        frame: CallFrame32,
        result: Result<u32, E>,
    ) -> Result<(), HostError> {
        let value = match result {
            Ok(value) => value,
            Err(E::Memory(e)) => return Err(HostError::Memory(e)),
            Err(e) => {
                let thread = self.current_windows_thread()?.with_last_error(e.win32());
                self.publish_windows_thread(thread)?;
                0
            }
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(())
    }
}
fn version_error(error: &HostError) -> u32 {
    match error {
        HostError::Version(e) => e.win32(),
        HostError::File(e) => e.win32(),
        HostError::Module(crate::loader::ModuleError32::MissingModule) => 2,
        _ => 87,
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
