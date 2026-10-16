use super::*;
use crate::{
    memory::{Access, GuestAddress},
    process::{CallError, EngineInstance, HostError},
    windows::thread::ThreadApi32,
};

impl EngineInstance {
    pub fn configure_windows_fls(&mut self, indices: u32) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some()
            || self.callback.is_some()
            || !self.threads.fls.context.is_empty()
            || !self.threads.fls.replies.is_empty()
        {
            return Err(HostError::Thread(ThreadError32::Busy));
        }
        let next =
            FlsRuntime32::new(indices, self.threads.limits.threads).map_err(HostError::Thread)?;
        self.threads.fls = next;
        Ok(())
    }
    pub(in crate::process) fn windows_fls_provider_owns_token(&self, token: u32) -> bool {
        self.threads
            .fls
            .replies
            .iter()
            .any(|reply| reply.thread == self.threads.current_id() && reply.token == token)
    }
    pub(in crate::process) fn windows_fls_owns_token(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> bool {
        self.threads
            .fls
            .replies
            .iter()
            .any(|reply| reply.owner == owner && reply.token == token)
    }
    pub(in crate::process::windows::threads) fn complete_windows_fls_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: ThreadApi32,
        frame: CallFrame32,
    ) -> Result<(), HostError> {
        let thread = self.current_windows_thread_id()?;
        if self.windows_fls_owns_token(owner, token) {
            let reply = self
                .threads
                .fls
                .replies
                .iter()
                .find(|reply| {
                    reply.thread == thread && reply.owner == owner && reply.token == token
                })
                .ok_or(HostError::Thread(ThreadError32::Stale))?;
            if api != ThreadApi32::FlsFree || reply.frame != frame {
                return Err(HostError::Call(CallError::InvalidRequest));
            }
            let operation = reply.operation;
            self.advance_windows_fls_operation(operation)?;
            return Ok(());
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        if self.module_teb_address().is_some() {
            self.memory()?
                .ensure_transaction_versions(1)
                .map_err(HostError::Memory)?;
        }
        let args = frame.arguments();
        let context = self.current_windows_thread()?;
        let result = match api {
            ThreadApi32::FlsAlloc => {
                if args[0] != 0 {
                    self.memory()?
                        .check_span(GuestAddress(args[0]), 1, Access::Execute)
                        .map_err(HostError::Memory)?;
                }
                self.threads.fls.context.allocate(thread, args[0])
            }
            ThreadApi32::FlsGetValue => self.threads.fls.context.get(thread, args[0]),
            ThreadApi32::FlsSetValue => self
                .threads
                .fls
                .context
                .set(thread, args[0], args[1])
                .map(|()| 1),
            ThreadApi32::FlsFree => {
                if self.threads.fls.replies.len() == 32 {
                    Err(ThreadError32::Capacity)
                } else {
                    match self.threads.fls.context.begin_free(thread, args[0]) {
                        Ok(operation) => {
                            self.threads.fls.replies.push(FlsReply32 {
                                thread,
                                owner,
                                token,
                                frame,
                                operation,
                            });
                            self.advance_windows_fls_operation(operation)?;
                            return Ok(());
                        }
                        Err(error) => Err(error),
                    }
                }
            }
            _ => unreachable!(),
        };
        let (value, error) = match result {
            Ok(value) => (
                value,
                if api == ThreadApi32::FlsGetValue {
                    Some(0)
                } else {
                    None
                },
            ),
            Err(error) => (
                if api == ThreadApi32::FlsAlloc {
                    u32::MAX
                } else {
                    0
                },
                Some(error.win32()),
            ),
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        self.publish_windows_thread(error.map_or(context, |error| context.with_last_error(error)))?;
        self.publish_prepared_call(prepared);
        Ok(())
    }
}
