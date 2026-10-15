use super::{Reply32, decoding};
use crate::{
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32, WindowsApi32,
        synchronization::{
            MessageWait32, SynchronizationError32 as Error, WAIT_FAILED, WaitMode32, WaitOutcome32,
            WaitResult32,
        },
    },
};

impl EngineInstance {
    pub(in crate::process) fn try_complete_synchronization_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let kernel =
            WindowsApi32::from_id(id).filter(|api| (0x0001_0011..=0x0001_0020).contains(&api.id()));
        let message_wait = matches!(id, 0x0002_0029 | 0x0002_002a | 0x0002_0059);
        if kernel.is_none() && !message_wait {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        let info = crate::process::windows_gate_info(id).ok_or(HostError::InvalidRequest)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != info.stack_words
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        self.checked_pending_call(owner, token)?;
        if let Some(reply) = self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
        {
            if reply.owner != owner || reply.token != token || reply.frame != frame {
                return Err(HostError::Call(CallError::Busy));
            }
            self.finish_synchronization_reply()?;
            return Ok(true);
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let (_, now) = self.cpu_clock.monotonic_bounds()?;
        self.synchronization_mut()?
            .observe_time(now)
            .map_err(HostError::Synchronization)?;
        let outcome = if let Some(api) = kernel {
            self.execute_synchronization_call(api, frame.arguments())
        } else {
            self.execute_message_wait(id, frame.arguments())
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(HostError::Synchronization(error)) => WaitOutcome32::Ready(WaitResult32 {
                value: if id != 0x0002_0059 && (kernel.is_some_and(is_wait) || message_wait) {
                    WAIT_FAILED
                } else {
                    0
                },
                last_error: Some(error.win32()),
            }),
            Err(error) => return Err(error),
        };
        let (wait, result) = match outcome {
            WaitOutcome32::Ready(result) => (None, Some(result)),
            WaitOutcome32::Pending(wait) => (Some(wait), None),
        };
        let (deadline, alertable, message) = wait
            .map(|wait| {
                self.synchronization
                    .as_ref()
                    .unwrap()
                    .context
                    .wait_details(wait)
                    .unwrap()
            })
            .unwrap_or((None, false, None));
        self.synchronization.as_mut().unwrap().reply = Some(Reply32 {
            owner,
            token,
            frame,
            wait,
            result,
            deadline,
            alertable,
            wake_mask: message.map_or(0, |message| message.mask),
            boolean_result: id == 0x0002_0059,
        });
        self.finish_synchronization_reply()?;
        Ok(true)
    }
    pub(super) fn finish_synchronization_reply(&mut self) -> Result<(), HostError> {
        let mut reply = self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, reply.token)?;
        if reply.result.is_none() {
            let wait = reply.wait.ok_or(HostError::Infrastructure)?;
            let result = self
                .synchronization
                .as_mut()
                .unwrap()
                .context
                .take_wait_observing_messages(wait)
                .map_err(HostError::Synchronization)?;
            let Some(result) = result else {
                return Ok(());
            };
            if reply.wake_mask != 0
                && let Some(context) = self.user32.as_mut()
            {
                context.observe_wait_queue(reply.wake_mask);
            }
            reply.result = Some(result);
            self.synchronization.as_mut().unwrap().reply = Some(reply);
        }
        let result = reply.result.unwrap();
        let value = if reply.boolean_result {
            u32::from(result.last_error.is_none())
        } else {
            result.value
        };
        let prepared = Self::prepare_completed_call(reply.frame.complete(value))?;
        if let Some(error) = result.last_error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(prepared);
        self.synchronization.as_mut().unwrap().reply = None;
        Ok(())
    }
    fn execute_synchronization_call(
        &mut self,
        api: WindowsApi32,
        args: &[u32],
    ) -> Result<WaitOutcome32, HostError> {
        use WindowsApi32::*;
        let success =
            |value, last_error| Ok(WaitOutcome32::Ready(WaitResult32 { value, last_error }));
        match api {
            CreateEventA | CreateEventW => {
                let inherit = decoding::inherit(self.memory()?, args[0])?;
                let name = if args[3] == 0 {
                    None
                } else {
                    Some(decoding::name(
                        self.memory()?,
                        args[3],
                        api == CreateEventW,
                    )?)
                };
                if let Some(name) = name.as_deref()
                    && self
                        .file_mapping_name_conflict(name)
                        .map_err(HostError::Synchronization)?
                {
                    return Err(HostError::Synchronization(Error::InvalidHandle));
                }
                let (handle, existed) = self
                    .synchronization_mut()?
                    .create_event(args[1] != 0, args[2] != 0, inherit, name.as_deref())
                    .map_err(HostError::Synchronization)?;
                success(handle, Some(if existed { 183 } else { 0 }))
            }
            OpenEventA | OpenEventW => {
                if args[2] == 0 {
                    return Err(HostError::Synchronization(Error::InvalidName));
                }
                let name = decoding::name(self.memory()?, args[2], api == OpenEventW)?;
                if self
                    .file_mapping_name_conflict(&name)
                    .map_err(HostError::Synchronization)?
                {
                    return Err(HostError::Synchronization(Error::InvalidHandle));
                }
                let handle = self
                    .synchronization_mut()?
                    .open_event(args[0], args[1] != 0, &name)
                    .map_err(HostError::Synchronization)?;
                success(handle, None)
            }
            SetEvent | ResetEvent | PulseEvent | CloseHandle => {
                let context = self.synchronization_mut()?;
                match api {
                    SetEvent => context.set_event(args[0]),
                    ResetEvent => context.reset_event(args[0]),
                    PulseEvent => context.pulse_event(args[0]),
                    CloseHandle => context.close_handle(args[0]),
                    _ => unreachable!(),
                }
                .map_err(HostError::Synchronization)?;
                success(1, None)
            }
            DuplicateHandle => self.duplicate_event_call(args),
            WaitForSingleObject | WaitForSingleObjectEx => {
                let mut handles = [args[0]];
                self.with_kernel_wait_handles(&mut handles, |engine, handles| {
                    engine
                        .synchronization_mut()?
                        .wait(
                            handles,
                            WaitMode32::Any,
                            args[1],
                            api == WaitForSingleObjectEx && args[2] != 0,
                            None,
                        )
                        .map_err(HostError::Synchronization)
                })
            }
            WaitForMultipleObjects | WaitForMultipleObjectsEx => {
                let (mut handles, count) = decoding::handles(self.memory()?, args[1], args[0])?;
                let original = handles;
                self.with_kernel_wait_handles(&mut handles[..count], |engine, handles| {
                    engine
                        .synchronization_mut()?
                        .wait_projected(
                            crate::windows::synchronization::WaitHandles32 {
                                original: &original[..count],
                                projected: handles,
                            },
                            if args[2] != 0 {
                                WaitMode32::All
                            } else {
                                WaitMode32::Any
                            },
                            args[3],
                            api == WaitForMultipleObjectsEx && args[4] != 0,
                            None,
                        )
                        .map_err(HostError::Synchronization)
                })
            }
            Sleep | SleepEx => self
                .synchronization_mut()?
                .wait(
                    &[],
                    WaitMode32::Sleep,
                    args[0],
                    api == SleepEx && args[1] != 0,
                    None,
                )
                .map_err(HostError::Synchronization),
            SignalObjectAndWait => {
                self.synchronization()?
                    .prepare_signal_handles(&[args[0]])
                    .map_err(HostError::Synchronization)?;
                let mut handles = [args[1]];
                self.with_kernel_wait_handles(&mut handles, |engine, handles| {
                    engine
                        .synchronization_mut()?
                        .signal_and_wait(args[0], handles[0], args[2], args[3] != 0)
                        .map_err(HostError::Synchronization)
                })
            }
            _ => Err(HostError::InvalidRequest),
        }
    }
    fn duplicate_event_call(&mut self, args: &[u32]) -> Result<WaitOutcome32, HostError> {
        if args[0] != u32::MAX || args[2] != u32::MAX && !(args[2] == 0 && args[6] & 1 != 0) {
            return Err(HostError::Synchronization(Error::InvalidHandle));
        }
        if args[6] & !3 != 0 {
            return Err(HostError::Synchronization(Error::InvalidWait));
        }
        let target = args[2] != 0;
        if target {
            decoding::check_output(self.memory()?, args[3])?;
        }
        let prepared = self.synchronization()?.prepare_duplicate(
            args[1],
            args[4],
            args[5] != 0,
            args[6] & 2 != 0,
            args[6] & 1 != 0,
            target,
        );
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if args[6] & 1 != 0 {
                    let _ = self.synchronization_mut()?.close_handle(args[1]);
                }
                return Err(HostError::Synchronization(error));
            }
        };
        if target && args[3] != 0 {
            self.memory
                .as_mut()
                .ok_or(HostError::Closed)?
                .write(
                    GuestAddress(args[3]),
                    &prepared.handle().unwrap().to_le_bytes(),
                )
                .map_err(HostError::Memory)?;
        }
        self.synchronization_mut()?.commit_duplicate(prepared);
        Ok(WaitOutcome32::Ready(WaitResult32 {
            value: 1,
            last_error: None,
        }))
    }
    fn execute_message_wait(&mut self, id: u32, args: &[u32]) -> Result<WaitOutcome32, HostError> {
        self.refresh_user32_input()?;
        let (count, address, timeout, mask, flags) = if id == 0x0002_0059 {
            (0, 0, u32::MAX, 0x4ff, 0)
        } else if id == 0x0002_002a {
            (args[0], args[1], args[2], args[3], args[4])
        } else {
            (args[0], args[1], args[3], args[4], u32::from(args[2] != 0))
        };
        if flags & !7 != 0 || mask & !0x7fff != 0 || count > 63 {
            return Err(HostError::Synchronization(Error::InvalidWait));
        }
        let (mut handles, count) = decoding::handles(self.memory()?, address, count)?;
        let original = handles;
        let result = self.with_kernel_wait_handles(&mut handles[..count], |engine, handles| {
            let queue = engine
                .user32
                .as_ref()
                .ok_or(HostError::Synchronization(Error::Unsupported))?
                .wait_queue_status();
            engine
                .synchronization_mut()?
                .wait_with_messages_projected(
                    crate::windows::synchronization::WaitHandles32 {
                        original: &original[..count],
                        projected: handles,
                    },
                    if flags & 1 != 0 {
                        WaitMode32::All
                    } else {
                        WaitMode32::Any
                    },
                    timeout,
                    flags & 2 != 0,
                    MessageWait32 {
                        mask,
                        input_available: flags & 4 != 0,
                    },
                    queue,
                )
                .map_err(HostError::Synchronization)
        })?;
        self.user32.as_mut().unwrap().observe_wait_queue(mask);
        Ok(result)
    }
}
fn is_wait(api: WindowsApi32) -> bool {
    matches!(
        api,
        WindowsApi32::WaitForSingleObject
            | WindowsApi32::WaitForSingleObjectEx
            | WindowsApi32::WaitForMultipleObjects
            | WindowsApi32::WaitForMultipleObjectsEx
            | WindowsApi32::SignalObjectAndWait
    )
}
