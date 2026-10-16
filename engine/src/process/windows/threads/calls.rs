use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32,
        thread::{ThreadApi32, ThreadError32},
    },
};

impl EngineInstance {
    fn schedule_after_windows_thread_completion(&mut self) -> Result<(), HostError> {
        if self.pending_call.is_none()
            && self.threads.threads.iter().any(|thread| {
                thread.id == self.threads.current_id()
                    && thread.state != crate::windows::thread::ThreadState32::Terminated
                    && !thread.active_invocations.is_empty()
            })
        {
            return Ok(());
        }
        if self.windows_service.is_some() {
            self.threads.schedule_required = Some(self.threads.current_id());
        } else {
            let _ = self.schedule_windows_thread()?;
        }
        Ok(())
    }

    pub(in crate::process) fn try_complete_thread_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let private = matches!(id, super::THREAD_RETURN_ID | super::INVOCATION_RETURN_ID);
        let api = ThreadApi32::from_id(id).filter(|api| ThreadApi32::core_apis().contains(api));
        if api.is_none() && !private {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        let pending = self.checked_pending_call(owner, token)?;
        if pending.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.map_or(0, ThreadApi32::stack_words)
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        if let Some(
            api @ (ThreadApi32::FlsAlloc
            | ThreadApi32::FlsFree
            | ThreadApi32::FlsGetValue
            | ThreadApi32::FlsSetValue),
        ) = api
        {
            self.complete_windows_fls_call(owner, token, api, frame)?;
            return Ok(true);
        }
        if id == super::INVOCATION_RETURN_ID {
            self.finish_thread_invocation(frame)?;
            return Ok(true);
        }
        if id == super::THREAD_RETURN_ID || api == Some(ThreadApi32::ExitThread) {
            if private {
                let info = self
                    .threads
                    .info(self.threads.current_id())
                    .map_err(HostError::Thread)?;
                let expected = info
                    .stack_base
                    .checked_add(info.stack_pages * 4096)
                    .and_then(|value| value.checked_sub(4));
                if expected != Some(frame.state().registers[4]) {
                    return Err(HostError::Thread(ThreadError32::InvalidParameter));
                }
            }
            let code = if private {
                frame.state().registers[0]
            } else {
                frame.arguments()[0]
            };
            self.request_windows_thread_exit(code)?;
            if self.exit_code.is_none() {
                self.schedule_after_windows_thread_completion()?;
            }
            return Ok(true);
        }
        let api = api.unwrap();
        if api == ThreadApi32::TerminateThread
            && self.threads.is_last_live()
            && self.resolve_thread_handle(frame.arguments()[0], 1).ok()
                == Some(self.threads.current_id())
        {
            if self.finalize_forced_last_windows_thread(frame.arguments()[1])? {
                self.threads.retire_process_threads(frame.arguments()[1]);
            }
            return Ok(true);
        }
        if api == ThreadApi32::SwitchToThread {
            let switched = self.prepare_windows_thread_yield()?;
            let returned =
                Self::prepare_completed_call(frame.complete(u32::from(switched.is_some())))?;
            self.publish_prepared_call(returned);
            if let Some(switched) = switched {
                self.commit_windows_thread_yield(switched);
            }
            return Ok(true);
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        let thread = self.current_windows_thread()?;
        if self.module_teb_address().is_some() {
            self.memory()?
                .ensure_transaction_versions(1)
                .map_err(HostError::Memory)?;
        }
        use ThreadApi32::*;
        let args = frame.arguments();
        let result = (|| -> Result<u32, HostError> {
            match api {
                CreateThread => self.create_windows_thread(args),
                OpenThread => self.open_windows_thread(args[2], args[0], args[1] != 0),
                GetExitCodeThread => {
                    let id = self.resolve_query_thread(args[0])?;
                    let info = self.threads.info(id).map_err(HostError::Thread)?;
                    self.memory
                        .as_mut()
                        .unwrap()
                        .write_words32(&[crate::memory::WordWrite32 {
                            address: crate::memory::GuestAddress(args[1]),
                            value: info.exit_code,
                        }])
                        .map_err(HostError::Memory)?;
                    Ok(1)
                }
                SuspendThread => self.change_thread_suspend(args[0], false),
                ResumeThread => self.change_thread_suspend(args[0], true),
                TerminateThread => {
                    let id = self.resolve_thread_handle(args[0], 1)?;
                    self.terminate_windows_thread(id, args[1])?;
                    Ok(1)
                }
                SwitchToThread => unreachable!(),
                TlsAlloc => self.allocate_windows_tls(),
                TlsFree => self.free_windows_tls(args[0]).map(|()| 1),
                TlsGetValue => self.get_windows_tls(args[0]),
                TlsSetValue => self.set_windows_tls(args[0], args[1]).map(|()| 1),
                DisableThreadLibraryCalls
                | GetThreadPriority
                | SetThreadPriority
                | GetThreadTimes => self.execute_thread_utility(api, args),
                GetThreadContext | SetThreadContext => self.execute_thread_context(api, args),
                ExitThread | FlsAlloc | FlsFree | FlsGetValue | FlsSetValue => unreachable!(),
            }
        })();
        if result.is_ok()
            && self
                .threads
                .info(self.threads.current_id())
                .is_ok_and(|info| info.state == crate::windows::thread::ThreadState32::Terminated)
        {
            if self.exit_code.is_none() {
                self.schedule_after_windows_thread_completion()?;
            }
            return Ok(true);
        }
        let (value, error) = match result {
            Ok(u32::MAX) if api == TlsAlloc => (u32::MAX, Some(ThreadError32::Capacity.win32())),
            Ok(value) => (value, if api == TlsGetValue { Some(0) } else { None }),
            Err(HostError::Synchronization(error)) => (
                if api == GetThreadPriority {
                    0x7fff_ffff
                } else if matches!(api, SuspendThread | ResumeThread) {
                    u32::MAX
                } else {
                    0
                },
                Some(error.win32()),
            ),
            Err(HostError::Thread(error)) => (
                if api == GetThreadPriority {
                    0x7fff_ffff
                } else if matches!(api, TlsAlloc | SuspendThread | ResumeThread) {
                    u32::MAX
                } else {
                    0
                },
                Some(error.win32()),
            ),
            Err(error) => return Err(error),
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        self.publish_windows_thread(error.map_or(thread, |error| thread.with_last_error(error)))?;
        self.publish_prepared_call(prepared);
        if api == SuspendThread
            && self
                .threads
                .info(self.threads.current_id())
                .is_ok_and(|thread| thread.suspend_count != 0)
        {
            self.threads.schedule_required = Some(self.threads.current_id());
        }
        Ok(true)
    }
}
