use super::*;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};
use std::collections::VecDeque;

pub(super) struct ThreadInitialization32 {
    pub calls: VecDeque<(GuestAddress, [u32; 3])>,
    reason: u32,
    exit_code: u32,
    loader_owned: bool,
    prepared: bool,
    pub(super) active: Option<u64>,
}
impl EngineInstance {
    pub(super) fn request_windows_thread_exit(&mut self, code: u32) -> Result<(), HostError> {
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == self.threads.current_id())
            .unwrap();
        self.threads.threads[index].exit_requested = Some(code);
        if self.threads.threads[index]
            .lifecycle
            .as_ref()
            .is_some_and(|phase| phase.reason == 2)
        {
            let owned = self.threads.threads[index]
                .lifecycle
                .as_ref()
                .unwrap()
                .loader_owned;
            self.threads.threads[index].lifecycle = Some(ThreadInitialization32 {
                calls: VecDeque::new(),
                reason: 3,
                prepared: false,
                exit_code: code,
                loader_owned: owned,
                active: None,
            });
        }
        if self
            .loader_initialization_owner()
            .is_some_and(|owner| owner != self.threads.current_id())
        {
            self.threads.threads[index].state = ThreadState32::Waiting;
            return Ok(());
        }
        self.advance_windows_thread_initialization()?;
        Ok(())
    }
    pub(super) fn advance_windows_thread_initialization(&mut self) -> Result<bool, HostError> {
        let id = self.threads.current_id();
        let index = self
            .threads
            .threads
            .iter()
            .position(|thread| thread.id == id)
            .unwrap();
        if self.threads.threads[index]
            .lifecycle
            .as_ref()
            .is_some_and(|phase| phase.active.is_some())
        {
            return Ok(false);
        }
        if self.threads.threads[index].exit_requested.is_some()
            && !self.advance_windows_fls_thread_exit()?
        {
            return Ok(true);
        }
        if self.threads.threads[index].lifecycle.is_none()
            && (!self.threads.threads[index].started
                || self.threads.threads[index].exit_requested.is_some())
        {
            self.acquire_loader_thread()?;
            let reason = if self.threads.threads[index].exit_requested.is_some() {
                3
            } else {
                2
            };
            let calls = match self.windows_thread_initializers(reason) {
                Ok(calls) => calls,
                Err(error) => {
                    self.release_idle_loader_thread();
                    return Err(error);
                }
            };
            self.threads.threads[index].lifecycle = Some(ThreadInitialization32 {
                calls: calls.into(),
                reason,
                prepared: true,
                exit_code: self.threads.threads[index].exit_requested.unwrap_or(0),
                loader_owned: true,
                active: None,
            });
        }
        if self.threads.threads[index]
            .lifecycle
            .as_ref()
            .is_some_and(|phase| !phase.loader_owned)
        {
            self.acquire_loader_thread()?;
            self.threads.threads[index]
                .lifecycle
                .as_mut()
                .unwrap()
                .loader_owned = true;
        }
        if self.threads.threads[index]
            .lifecycle
            .as_ref()
            .is_some_and(|phase| !phase.prepared)
        {
            let calls = self.windows_thread_initializers(3)?;
            let phase = self.threads.threads[index].lifecycle.as_mut().unwrap();
            phase.calls = calls.into();
            phase.prepared = true;
        }
        let Some(initialization) = self.threads.threads[index].lifecycle.as_ref() else {
            return Ok(false);
        };
        if let Some(&(procedure, args)) = initialization.calls.front() {
            let next = self
                .threads
                .next_invocation
                .checked_add(1)
                .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
            let mut arguments = [0; 16];
            arguments[..3].copy_from_slice(&args);
            let result = self.enter_thread_invocation(super::invocation::ThreadInvocation32 {
                id: next,
                target: id,
                procedure,
                arguments,
                count: 3,
                completion: ThreadInvocationCompletion32::Initialize,
                timer: None,
                activation: None,
                managed: None,
            });
            if let Err(error) = result {
                self.release_idle_loader_thread();
                self.threads.threads[index]
                    .lifecycle
                    .as_mut()
                    .unwrap()
                    .loader_owned = false;
                return Err(error);
            }
            self.threads.next_invocation = next;
            self.threads.threads[index]
                .lifecycle
                .as_mut()
                .unwrap()
                .active = Some(next);
            return Ok(true);
        }
        let reason = initialization.reason;
        let code = initialization.exit_code;
        if reason == 3 && self.threads.is_last_live() {
            if !self.finalize_last_windows_thread(code)? {
                return Ok(false);
            }
            self.threads.retire_process_threads(code);
        } else if reason == 3 {
            self.terminate_windows_thread(id, code)?;
        } else {
            self.threads.threads[index].started = true;
        }
        self.threads.threads[index].lifecycle = None;
        self.threads.threads[index].exit_requested = None;
        self.release_idle_loader_thread();
        Ok(false)
    }
}
