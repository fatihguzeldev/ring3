use crate::{
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        native::{NativeApi32, NativeDebugOutput32},
    },
};
impl EngineInstance {
    pub fn last_windows_debug_output(&self) -> Option<&NativeDebugOutput32> {
        self.native_diagnostics.last()
    }
    pub(in crate::process) fn try_complete_native_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = NativeApi32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let args = frame.arguments();
        let value = match api {
            NativeApi32::LdrAddRefDll => {
                if args[0] & !1 != 0
                    || !self
                        .loaded_windows_modules()
                        .any(|module| module.base() == args[1])
                {
                    0xc000000d
                } else {
                    match self.prepare_native_module_reference(args[1], args[0] != 0) {
                        Ok(Some(reference)) => {
                            Self::prepare_completed_call(frame.complete(0))?;
                            self.commit_module_handle_reference(reference);
                            0
                        }
                        Ok(None) => 0xc000000d,
                        Err(HostError::Module(
                            crate::loader::ModuleError32::ReferenceExhausted,
                        )) => 0xc000009a,
                        Err(error) => return Err(error),
                    }
                }
            }
            NativeApi32::NtQuerySystemTime => {
                let value = self.windows_filetime()?;
                Self::prepare_completed_call(frame.complete(0))?;
                match self
                    .memory
                    .as_mut()
                    .unwrap()
                    .write(GuestAddress(args[0]), &value.to_le_bytes())
                {
                    Ok(()) => 0,
                    Err(crate::memory::MemoryError::Fault(_)) => 0xc0000005,
                    Err(error) => return Err(HostError::Memory(error)),
                }
            }
            NativeApi32::VDbgPrintExWithPrefix => {
                let Some(sequence) = self.native_diagnostics.next() else {
                    self.publish_prepared_call(Self::prepare_completed_call(
                        frame.complete(0xc0000095),
                    )?);
                    return Ok(true);
                };
                let formatted =
                    match super::formatting::format(self.memory()?, args[0], args[3], args[4]) {
                        Ok(formatted) => formatted,
                        Err(HostError::InvalidRequest) => Err(0xc000000d),
                        Err(error) => return Err(error),
                    };
                match formatted {
                    Ok((bytes, length)) => {
                        Self::prepare_completed_call(frame.complete(0))?;
                        self.native_diagnostics
                            .commit(sequence, args[1], args[2], bytes, length);
                        0
                    }
                    Err(status) => status,
                }
            }
        };
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
}
