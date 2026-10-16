use super::{FileStage32, MAX_FILE_BYTES, PendingEffectFile32, guest};
use crate::{
    files::FileIo32,
    input::dinput::{DirectInputResult32 as H, MethodGate},
    memory::{Access, GuestAddress},
    process::{
        CallError, EngineInstance, HostError, WindowsProviderOutcome32, calls::call::PendingOwner,
    },
    windows::CallFrame32,
};

impl EngineInstance {
    pub(in crate::process) fn try_complete_dinput_file_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        gate: MethodGate,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        if !gate.interface.device() || !matches!(gate.slot, 27 | 28) {
            return Ok(false);
        }
        let args = frame.arguments();
        let pointer = GuestAddress(args[0]);
        let code_page = self.windows_ansi_code_page();
        let runtime = self.dinput.as_ref().ok_or(HostError::DirectInput(
            super::super::super::DirectInputError32::NotConfigured,
        ))?;
        if let Some(pending) = &runtime.file_call {
            if (pending.owner, pending.token, pending.gate, pending.frame)
                != (owner, token, gate, frame)
            {
                return Err(HostError::Call(CallError::Busy));
            }
            return self.complete_dinput_file_progress();
        }
        let input = (|| -> Result<_, H> {
            let memory = self.memory.as_ref().ok_or(H::NOT_INITIALIZED)?;
            runtime.com.preflight(memory, pointer)?;
            if !runtime
                .object(pointer)?
                .interface
                .compatible(gate.interface)
            {
                return Err(H::INVALID_PARAM);
            }
            let path = guest::filename(memory, args[1], gate.interface.wide(), code_page)?;
            let bytes = if gate.slot == 28 {
                Some(guest::snapshot(memory, args[3], args[2], args[4])?)
            } else {
                if args[4] & !0x11 != 0 || args[2] == 0 {
                    return Err(H::INVALID_PARAM);
                }
                memory.check_span(GuestAddress(args[2]), 1, Access::Execute)?;
                None
            };
            Ok((path, bytes))
        })();
        let (path, bytes) = match input {
            Ok(input) => input,
            Err(error) => {
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(error.0))?);
                return Ok(true);
            }
        };
        if let Some(bytes) = bytes {
            let value = self
                .replace_provider_file(&path, &bytes)
                .map_or_else(file_error, |_| 0);
            self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
            return Ok(true);
        }
        let lease = match self.open_provider_file(&path, false) {
            Ok(lease) => lease,
            Err(error) => {
                self.publish_prepared_call(Self::prepare_completed_call(
                    frame.complete(file_error(error)),
                )?);
                return Ok(true);
            }
        };
        let stage = match self.provider_file_size(lease) {
            Ok(length) if length <= MAX_FILE_BYTES as u64 => FileStage32::Read(length as usize),
            Ok(_) => FileStage32::Failed(H::OUT_OF_MEMORY.0),
            Err(error) => FileStage32::Failed(file_error(error)),
        };
        self.dinput.as_mut().unwrap().file_call = Some(PendingEffectFile32 {
            owner,
            token,
            gate,
            frame,
            lease: Some(lease),
            request: None,
            stage,
        });
        self.complete_dinput_file_progress()
    }
    fn complete_dinput_file_progress(&mut self) -> Result<bool, HostError> {
        let mut pending = self.dinput.as_mut().unwrap().file_call.take().unwrap();
        let result = self.progress_dinput_file(&mut pending);
        let publication = match result {
            Ok(Some(WindowsProviderOutcome32::Return(value))) => {
                Self::prepare_completed_call(pending.frame.complete(value))
                    .map(|prepared| self.publish_prepared_call(prepared))
            }
            Ok(Some(WindowsProviderOutcome32::Callback(request))) => {
                self.park_media_callback(request)
            }
            Ok(None) => {
                self.dinput.as_mut().unwrap().file_call = Some(pending);
                return Ok(true);
            }
            Err(error) => Err(error),
        };
        if let Err(error) = publication {
            self.dinput.as_mut().unwrap().file_call = Some(pending);
            return Err(error);
        }
        Ok(true)
    }
    fn progress_dinput_file(
        &mut self,
        pending: &mut PendingEffectFile32,
    ) -> Result<Option<WindowsProviderOutcome32>, HostError> {
        if let FileStage32::Read(length) = pending.stage {
            match self.read_provider_file(pending.lease.unwrap(), 0, length) {
                Ok(FileIo32::NeedBytes(request)) => {
                    pending.request = Some(request);
                    return Ok(None);
                }
                Ok(FileIo32::Ready(bytes)) => {
                    pending.stage = FileStage32::Ready(bytes);
                    pending.request = None
                }
                Err(error) => pending.stage = FileStage32::Failed(file_error(error)),
            }
        }
        if let Some(lease) = pending.lease {
            self.close_provider_file(lease)?;
            pending.lease = None;
        }
        let outcome = match &pending.stage {
            FileStage32::Failed(value) => WindowsProviderOutcome32::Return(*value),
            FileStage32::Ready(bytes) => {
                let args = pending.frame.arguments();
                self.dinput
                    .as_mut()
                    .unwrap()
                    .enumerate_effect_file(
                        self.memory.as_mut().unwrap(),
                        GuestAddress(args[0]),
                        pending.gate.interface,
                        args,
                        bytes,
                    )
                    .unwrap_or_else(|error| WindowsProviderOutcome32::Return(error.0))
            }
            FileStage32::Complete(outcome) => *outcome,
            FileStage32::Read(_) => unreachable!(),
        };
        pending.stage = FileStage32::Complete(outcome);
        Ok(Some(outcome))
    }
}
fn file_error(error: HostError) -> u32 {
    match error {
        HostError::File(error) => 0x80070000 | (error.win32() & 0xffff),
        _ => H::INVALID_PARAM.0,
    }
}
