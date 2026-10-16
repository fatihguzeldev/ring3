use super::{Reply32, decoding};
use crate::{
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32, WindowsApi32,
        synchronization::{
            MessageWait32, SynchronizationApi32, SynchronizationError32 as Error, WAIT_FAILED,
            WaitMode32, WaitOutcome32, WaitResult32,
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
        let object = SynchronizationApi32::from_id(id).filter(|api| {
            SynchronizationApi32::object_apis().contains(api)
                || *api == SynchronizationApi32::QueueUserAPC
        });
        if kernel.is_none() && !message_wait && object.is_none() {
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
        let thread = self.current_windows_thread_id()?;
        self.synchronization_mut()?
            .set_current_thread(thread)
            .map_err(HostError::Synchronization)?;
        if self.synchronization()?.now_ns() != now
            || self
                .synchronization()?
                .next_timer_deadline()
                .is_some_and(|due| due <= now)
        {
            let clock = self.prepare_synchronization_clock(now)?;
            self.commit_synchronization_clock(clock);
        }
        let outcome = if let Some(api) = object {
            self.execute_object_synchronization_call(api, frame.arguments())
        } else if let Some(api) = kernel {
            self.execute_synchronization_call(api, frame.arguments())
        } else {
            self.execute_message_wait(id, frame.arguments())
        };
        if object == Some(SynchronizationApi32::InitializeCriticalSection)
            && matches!(
                &outcome,
                Err(HostError::Synchronization(
                    Error::Capacity | Error::Allocation
                )) | Err(HostError::WindowsMemory(
                    crate::windows::memory::MemoryError32::Capacity
                        | crate::windows::memory::MemoryError32::Allocation
                ))
            )
        {
            let mut original = self.windows_context_state()?;
            original.state = frame.complete(0);
            let record = crate::windows::exceptions::ExceptionRecord32::new(
                0xc000_0017,
                0,
                original.state.eip,
                &[],
            )
            .map_err(HostError::Exception)?;
            self.begin_exception_delivery(record, original, owner, None, None, None)?;
            return Ok(true);
        }
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(HostError::Thread(error)) => WaitOutcome32::Ready(WaitResult32 {
                value: 0,
                last_error: Some(error.win32()),
            }),
            Err(HostError::Synchronization(error)) => WaitOutcome32::Ready(WaitResult32 {
                value: if id != 0x0002_0059 && (kernel.is_some_and(is_wait) || message_wait) {
                    WAIT_FAILED
                } else {
                    0
                },
                last_error: Some(error.win32()),
            }),
            Err(HostError::WindowsMemory(crate::windows::memory::MemoryError32::Exhausted)) => {
                return Err(HostError::Memory(
                    crate::memory::MemoryError::VersionExhausted,
                ));
            }
            Err(HostError::WindowsMemory(error)) => WaitOutcome32::Ready(WaitResult32 {
                value: 0,
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
            .unwrap_or((None, call_alertable(kernel, id, frame.arguments()), None));
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
            critical: object
                .filter(|api| *api == SynchronizationApi32::EnterCriticalSection)
                .filter(|_| wait.is_some())
                .map(|_| GuestAddress(frame.arguments()[0])),
        });
        self.finish_synchronization_reply()?;
        Ok(true)
    }
    pub(in crate::process) fn finish_synchronization_reply(&mut self) -> Result<(), HostError> {
        let reply = self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.execute_pending_guarded_service(reply.owner, reply.token, |engine| {
            engine.advance_synchronization_reply()
        })
    }
    pub(in crate::process) fn check_guard_service_wait_reply(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        if self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .is_none_or(|reply| reply.owner != owner || reply.token != token)
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        Ok(())
    }
    pub(in crate::process) fn commit_guard_service_wait_reply(&mut self) {
        self.synchronization.as_mut().unwrap().reply = None;
    }

    fn advance_synchronization_reply(&mut self) -> Result<(), HostError> {
        let mut reply = self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, reply.token)?;
        if reply.alertable && self.has_pending_thread_apc(self.threads.current_id()) {
            return Ok(());
        }
        if reply.result.is_none() {
            if let Some(address) = reply.critical {
                self.check_critical(address)?;
                self.memory()?
                    .ensure_transaction_versions(1)
                    .map_err(HostError::Memory)?;
            }
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
                context.observe_wait_queue(self.threads.current_id(), reply.wake_mask);
            }
            reply.result = Some(result);
            self.synchronization.as_mut().unwrap().reply = Some(reply);
        }
        if let Some(address) = reply.critical {
            self.check_critical(address)?;
            self.memory()?
                .ensure_transaction_versions(1)
                .map_err(HostError::Memory)?;
            self.publish_critical_state(address);
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
        if !self.defer_guard_service_cleanup(
            crate::process::windows::service::WindowsServiceCleanup32::Wait,
        ) {
            self.synchronization.as_mut().unwrap().reply = None;
        }
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
                if api == CloseHandle && matches!(args[0], u32::MAX | 0xffff_fffe) {
                    return success(1, None);
                }
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
            DuplicateHandle => self.duplicate_windows_handle_call(args),
            WaitForSingleObject | WaitForSingleObjectEx => {
                let mut handles = [args[0]];
                self.with_kernel_wait_handles(&mut handles, |engine, handles| {
                    engine.with_windows_wait_objects(handles, |context| {
                        context.wait(
                            handles,
                            WaitMode32::Any,
                            args[1],
                            api == WaitForSingleObjectEx && args[2] != 0,
                            None,
                        )
                    })
                })
            }
            WaitForMultipleObjects | WaitForMultipleObjectsEx => {
                let (mut handles, count) = decoding::handles(self.memory()?, args[1], args[0])?;
                let original = handles;
                self.with_kernel_wait_handles(&mut handles[..count], |engine, handles| {
                    engine.with_windows_wait_objects(handles, |context| {
                        context.wait_projected(
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
                    })
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
                    engine.with_windows_wait_objects(handles, |context| {
                        context.signal_and_wait(args[0], handles[0], args[2], args[3] != 0)
                    })
                })
            }
            _ => Err(HostError::InvalidRequest),
        }
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
                .wait_queue_status(engine.current_windows_thread_id()?);
            engine.with_windows_wait_objects(handles, |context| {
                context.wait_with_messages_projected(
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
            })
        })?;
        let thread = self.current_windows_thread_id()?;
        self.user32
            .as_mut()
            .unwrap()
            .observe_wait_queue(thread, mask);
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

fn call_alertable(api: Option<WindowsApi32>, id: u32, args: &[u32]) -> bool {
    match api {
        Some(WindowsApi32::WaitForSingleObjectEx) => args[2] != 0,
        Some(WindowsApi32::WaitForMultipleObjectsEx) => args[4] != 0,
        Some(WindowsApi32::SleepEx) => args[1] != 0,
        Some(WindowsApi32::SignalObjectAndWait) => args[3] != 0,
        _ => id == 0x0002_002a && args[4] & 2 != 0,
    }
}
