use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{CallError, EngineInstance, HostError},
    windows::{
        CallFrame32,
        exceptions::{
            ExceptionApi32, ExceptionError32, HandlerId32,
            context::{CONTEXT_ALL32, CONTEXT_SIZE32, Win32Context32},
        },
    },
};
impl EngineInstance {
    pub(in crate::process) fn try_complete_exception_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = ExceptionApi32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != api.convention()
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let args = frame.arguments();
        use ExceptionApi32::*;
        match api {
            RaiseException => {
                if args[1] & !1 != 0 || args[2] > 15 {
                    return Err(HostError::Exception(ExceptionError32::InvalidParameter));
                }
                let mut information = [0; 15];
                let mut bytes = [0; 60];
                if args[2] != 0 {
                    self.memory()?
                        .read(GuestAddress(args[3]), &mut bytes[..args[2] as usize * 4])
                        .map_err(HostError::Memory)?;
                }
                for (word, bytes) in information.iter_mut().zip(bytes.chunks_exact(4)) {
                    *word = u32::from_le_bytes(bytes.try_into().unwrap());
                }
                let mut original = self.windows_context_state()?;
                original.state = frame.complete(0);
                let record = ExceptionRecord32::new(
                    args[0] & !0x1000_0000,
                    args[1],
                    original.state.eip,
                    &information[..args[2] as usize],
                )
                .map_err(HostError::Exception)?;
                self.begin_exception_delivery(record, original, owner, None, None, None)?;
            }
            RtlRaiseException | NtRaiseException => {
                let mut record = self.read_exception_record(args[0])?;
                let mut original = self.windows_context_state()?;
                original.state = frame.complete(0);
                if api == RtlRaiseException {
                    record.address = original.state.eip;
                }
                if api == NtRaiseException {
                    original = self.read_windows_context(args[1], &original)?;
                }
                if api == NtRaiseException && args[2] == 0 {
                    self.begin_exception_delivery_with_page_change(
                        record,
                        original,
                        owner,
                        None,
                        None,
                        ExceptionInitial32 {
                            last_chance: true,
                            ..ExceptionInitial32::default()
                        },
                    )?;
                } else {
                    self.begin_exception_delivery(record, original, owner, None, None, None)?;
                }
            }
            RtlCaptureContext => {
                let mut context = self.windows_context_state()?;
                context.state = frame.complete(context.state.registers[0]);
                let bytes = Win32Context32::capture(CONTEXT_ALL32, &context)
                    .map_err(|_| HostError::Exception(ExceptionError32::InvalidContext))?;
                let prepared = Self::prepare_completed_call(context.state)?;
                self.memory
                    .as_mut()
                    .unwrap()
                    .write_spans(&[(GuestAddress(args[0]), bytes.bytes().as_slice())])
                    .map_err(HostError::Memory)?;
                self.publish_prepared_call(prepared);
            }
            RtlRestoreContext | NtContinue => {
                let context = self.read_windows_context(args[0], &self.windows_context_state()?)?;
                let prepared = Self::prepare_completed_call(context.state)?;
                let apc = if api == NtContinue && args[1] != 0 {
                    self.prepare_windows_continue_apc(&context)?
                } else {
                    None
                };
                self.publish_windows_context(&context)?;
                self.publish_prepared_call(prepared);
                self.commit_windows_continue_apc(apc);
                // A nonlocal continuation abandons active exception dispatcher frames on this thread.
                let thread = self.threads.current_id();
                self.exceptions
                    .deliveries
                    .retain(|delivery| delivery.thread != thread);
            }
            RtlUnwind => {
                let mut original = self.windows_context_state()?;
                original.state = frame.complete(args[3]);
                let mut record = if args[2] == 0 {
                    ExceptionRecord32::new(0xc0000027, 0, original.state.eip, &[])
                        .map_err(HostError::Exception)?
                } else {
                    self.read_exception_record(args[2])?
                };
                record.flags |= 2;
                if args[0] == 0 {
                    record.flags |= 4;
                }
                self.begin_exception_delivery(
                    record,
                    original,
                    owner,
                    None,
                    Some(Unwind32 {
                        target: args[0],
                        return_value: args[3],
                    }),
                    None,
                )?;
            }
            UnhandledExceptionFilter => {
                let mut pointers = [0; 8];
                self.memory()?
                    .read(GuestAddress(args[0]), &mut pointers)
                    .map_err(HostError::Memory)?;
                let record = self
                    .read_exception_record(u32::from_le_bytes(pointers[..4].try_into().unwrap()))?;
                let mut bytes = [0; CONTEXT_SIZE32];
                self.memory()?
                    .read(
                        GuestAddress(u32::from_le_bytes(pointers[4..].try_into().unwrap())),
                        &mut bytes,
                    )
                    .map_err(HostError::Memory)?;
                let context = Win32Context32::from_bytes(&bytes)
                    .map_err(|_| HostError::Exception(ExceptionError32::InvalidContext))?;
                if self.exceptions.handlers.filter() == 0 {
                    self.publish_prepared_call(Self::prepare_completed_call(frame.complete(1))?);
                } else {
                    let mut original = self.windows_context_state()?;
                    original.state = frame.complete(0);
                    self.begin_exception_delivery(
                        record,
                        original,
                        owner,
                        None,
                        None,
                        Some(context),
                    )?;
                }
            }
            _ => {
                Self::prepare_completed_call(frame.complete(0))?;
                self.check_windows_thread_write()?;
                self.memory()?
                    .ensure_transaction_versions(1)
                    .map_err(HostError::Memory)?;
                let thread = self.current_windows_thread()?;
                let result = match api {
                    AddVectoredExceptionHandler | AddVectoredContinueHandler => {
                        self.memory()?
                            .check_span(GuestAddress(args[1]), 1, Access::Execute)
                            .map_err(HostError::Memory)?;
                        self.exceptions
                            .handlers
                            .add(api == AddVectoredContinueHandler, args[0] != 0, args[1])
                            .map(|id| id.0)
                    }
                    RemoveVectoredExceptionHandler | RemoveVectoredContinueHandler => {
                        Ok(u32::from(self.exceptions.handlers.remove(
                            api == RemoveVectoredContinueHandler,
                            HandlerId32(args[0]),
                        )))
                    }
                    SetUnhandledExceptionFilter => Ok(self.exceptions.handlers.set_filter(args[0])),
                    _ => unreachable!(),
                };
                let (value, error) = match result {
                    Ok(value) => (value, None),
                    Err(error) => (0, Some(error.win32())),
                };
                let prepared = Self::prepare_completed_call(frame.complete(value))?;
                self.publish_windows_thread(
                    error.map_or(thread, |error| thread.with_last_error(error)),
                )?;
                self.publish_prepared_call(prepared);
            }
        }
        Ok(true)
    }
    pub(super) fn read_exception_record(
        &self,
        address: u32,
    ) -> Result<ExceptionRecord32, HostError> {
        let mut bytes = [0; 80];
        self.memory()?
            .read(GuestAddress(address), &mut bytes)
            .map_err(HostError::Memory)?;
        let words: [u32; 20] = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        });
        let count = words[4];
        if count > 15 {
            return Err(HostError::Exception(ExceptionError32::InvalidParameter));
        }
        let mut record =
            ExceptionRecord32::new(words[0], words[1], words[3], &words[5..5 + count as usize])
                .map_err(HostError::Exception)?;
        record.nested = words[2];
        Ok(record)
    }
    fn read_windows_context(
        &self,
        address: u32,
        base: &crate::windows::exceptions::context::ContextState32,
    ) -> Result<crate::windows::exceptions::context::ContextState32, HostError> {
        let mut bytes = [0; CONTEXT_SIZE32];
        self.memory()?
            .read(GuestAddress(address), &mut bytes)
            .map_err(HostError::Memory)?;
        let context = Win32Context32::from_bytes(&bytes)
            .map_err(|_| HostError::Exception(ExceptionError32::InvalidContext))?;
        self.apply_windows_context(&context, base)
    }
}
impl EngineInstance {
    pub fn register_ntdll_module(&mut self, preferred_base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::exceptions::ntdll_module(preferred_base))
    }
}
