use super::{invocation::ThreadInvocation32, *};
use crate::{
    process::{EngineInstance, HostError},
    windows::synchronization::TimerApc32,
};

pub(in crate::process) struct PreparedThreadTimers32 {
    invocations: Vec<ThreadInvocation32>,
    next: u64,
}
impl EngineInstance {
    pub(in crate::process) fn prepare_thread_timer_apcs(
        &self,
        callbacks: &[TimerApc32],
    ) -> Result<PreparedThreadTimers32, HostError> {
        if self
            .threads
            .invocations
            .len()
            .checked_add(callbacks.len())
            .is_none_or(|count| count > self.threads.limits.apcs as usize)
        {
            return Err(HostError::Thread(ThreadError32::Capacity));
        }
        let mut invocations = Vec::new();
        invocations
            .try_reserve_exact(callbacks.len())
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        let mut next = self.threads.next_invocation;
        let (epoch, now) = self.cpu_clock.monotonic_bounds()?;
        let filetime_base = if callbacks.is_empty() {
            0
        } else {
            self.windows_filetime()?
                .checked_sub((now - epoch) / 100)
                .ok_or(HostError::Thread(ThreadError32::Exhausted))?
        };
        for &timer in callbacks {
            let thread = self.threads.info(timer.thread).map_err(HostError::Thread)?;
            if thread.state == ThreadState32::Terminated {
                return Err(HostError::Thread(ThreadError32::Stale));
            }
            next = next
                .checked_add(1)
                .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
            let filetime = filetime_base
                .checked_add(timer.due_ns.saturating_sub(epoch) / 100)
                .ok_or(HostError::Thread(ThreadError32::Exhausted))?;
            let mut arguments = [0; 16];
            arguments[..3].copy_from_slice(&[
                timer.argument,
                filetime as u32,
                (filetime >> 32) as u32,
            ]);
            invocations.push(ThreadInvocation32 {
                id: next,
                target: timer.thread,
                procedure: crate::memory::GuestAddress(timer.procedure),
                arguments,
                count: 3,
                completion: ThreadInvocationCompletion32::Apc,
                timer: Some(timer),
                activation: None,
                managed: self.waitable_timer_activation(timer.object, timer.generation),
            });
        }
        Ok(PreparedThreadTimers32 { invocations, next })
    }
    pub(in crate::process) fn commit_thread_timer_apcs(
        &mut self,
        prepared: PreparedThreadTimers32,
    ) {
        assert!(
            self.threads.invocations.len() + prepared.invocations.len()
                <= self.threads.invocations.capacity()
        );
        for invocation in prepared.invocations {
            self.threads.invocations.push(invocation);
        }
        self.threads.next_invocation = prepared.next;
    }
    pub(in crate::process) fn pending_apcs_without_timer(&self, thread: u32, object: u64) -> bool {
        self.threads.invocations.iter().any(|invocation| {
            invocation.target == thread
                && invocation.completion == ThreadInvocationCompletion32::Apc
                && invocation.timer.is_none_or(|timer| timer.object != object)
        })
    }
    pub(in crate::process) fn thread_apc_active(&self, thread: u32) -> bool {
        self.threads
            .threads
            .iter()
            .find(|record| record.id == thread)
            .is_some_and(|record| {
                record
                    .active_invocations
                    .iter()
                    .any(|invocation| invocation.completion == ThreadInvocationCompletion32::Apc)
            })
    }
    pub(in crate::process) fn cancel_thread_timer_apcs(&mut self, object: u64) {
        self.threads
            .invocations
            .retain(|invocation| invocation.timer.is_none_or(|timer| timer.object != object));
    }
}
