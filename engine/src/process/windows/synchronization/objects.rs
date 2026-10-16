use super::decoding;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::synchronization::{SynchronizationApi32 as Api, WaitOutcome32, WaitResult32},
};

impl EngineInstance {
    pub(super) fn execute_object_synchronization_call(
        &mut self,
        api: Api,
        args: &[u32],
    ) -> Result<WaitOutcome32, HostError> {
        let ready =
            |value, last_error| Ok(WaitOutcome32::Ready(WaitResult32 { value, last_error }));
        match api {
            Api::InitializeCriticalSection
            | Api::InitializeCriticalSectionAndSpinCount
            | Api::InitializeCriticalSectionEx
            | Api::DeleteCriticalSection
            | Api::EnterCriticalSection
            | Api::LeaveCriticalSection
            | Api::TryEnterCriticalSection
            | Api::SetCriticalSectionSpinCount => self.execute_critical_call(api, args),
            Api::CreateWaitableTimerA
            | Api::CreateWaitableTimerW
            | Api::OpenWaitableTimerA
            | Api::OpenWaitableTimerW
            | Api::SetWaitableTimer
            | Api::CancelWaitableTimer => self.execute_timer_call(api, args),
            Api::CreateMutexA
            | Api::CreateMutexW
            | Api::CreateSemaphoreA
            | Api::CreateSemaphoreW => {
                let mutex = matches!(api, Api::CreateMutexA | Api::CreateMutexW);
                let wide = matches!(api, Api::CreateMutexW | Api::CreateSemaphoreW);
                let inherit = decoding::inherit(self.memory()?, args[0])?;
                let name_address = args[if mutex { 2 } else { 3 }];
                let name = if name_address == 0 {
                    None
                } else {
                    Some(decoding::name(self.memory()?, name_address, wide)?)
                };
                let context = self.synchronization.as_ref().unwrap().context();
                let (prepared, existed) = if mutex {
                    context.prepare_mutex(args[1] != 0, inherit, name.as_deref())
                } else {
                    context.prepare_semaphore(
                        args[1] as i32,
                        args[2] as i32,
                        inherit,
                        name.as_deref(),
                    )
                }
                .map_err(HostError::Synchronization)?;
                let error = if existed { 183 } else { 0 };
                if self.module_teb_address().is_some()
                    && self.current_windows_thread()?.last_error() != error
                {
                    self.memory()?
                        .ensure_transaction_versions(1)
                        .map_err(HostError::Memory)?;
                }
                let handle = self
                    .synchronization
                    .as_mut()
                    .unwrap()
                    .context_mut()
                    .commit_object_handle(prepared);
                ready(handle, Some(error))
            }
            Api::OpenMutexA | Api::OpenMutexW | Api::OpenSemaphoreA | Api::OpenSemaphoreW => {
                let name = decoding::name(
                    self.memory()?,
                    args[2],
                    matches!(api, Api::OpenMutexW | Api::OpenSemaphoreW),
                )?;
                let context = self.synchronization.as_mut().unwrap().context_mut();
                let handle = if matches!(api, Api::OpenMutexA | Api::OpenMutexW) {
                    context.open_mutex(args[0], args[1] != 0, &name)
                } else {
                    context.open_semaphore(args[0], args[1] != 0, &name)
                }
                .map_err(HostError::Synchronization)?;
                ready(handle, None)
            }
            Api::QueueUserAPC => {
                let thread = self.resolve_thread_handle(args[1], 0x10)?;
                let prepared = self.prepare_thread_invocation(
                    thread,
                    GuestAddress(args[0]),
                    &[args[2]],
                    crate::process::windows::threads::ThreadInvocationCompletion32::Apc,
                )?;
                self.commit_thread_invocation(prepared);
                ready(1, None)
            }
            Api::ReleaseMutex => {
                self.synchronization
                    .as_mut()
                    .unwrap()
                    .context_mut()
                    .release_mutex(args[0])
                    .map_err(HostError::Synchronization)?;
                ready(1, None)
            }
            Api::ReleaseSemaphore => {
                decoding::check_output(self.memory()?, args[2])?;
                let (prepared, previous) = self
                    .synchronization
                    .as_ref()
                    .unwrap()
                    .context()
                    .prepare_release_semaphore(args[0], args[1] as i32)
                    .map_err(HostError::Synchronization)?;
                if args[2] != 0 {
                    self.memory
                        .as_mut()
                        .unwrap()
                        .write(GuestAddress(args[2]), &previous.to_le_bytes())
                        .map_err(HostError::Memory)?;
                }
                self.synchronization
                    .as_mut()
                    .unwrap()
                    .context_mut()
                    .commit_signals(prepared);
                ready(1, None)
            }
        }
    }
}
