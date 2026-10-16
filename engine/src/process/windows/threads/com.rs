use super::*;
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

pub(super) struct ComHost32 {
    pub thread: u32,
    handle: u32,
    sta: bool,
    origin: Option<u32>,
}

impl EngineInstance {
    pub(in crate::process) fn ensure_com_apartment_host(
        &mut self,
        sta: bool,
    ) -> Result<u32, HostError> {
        let caller = self.threads.current_id();
        let origin = self
            .ole32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .com_host_origin(caller, sta);
        if let Some(index) = self.com_host_candidate(sta, origin) {
            let (id, handle) = (
                self.threads.com_hosts[index].thread,
                self.threads.com_hosts[index].handle,
            );
            if self.threads.info(id).map_err(HostError::Thread)?.state != ThreadState32::Terminated
            {
                return Ok(id);
            }
            self.synchronization
                .as_mut()
                .ok_or(HostError::Infrastructure)?
                .context_mut()
                .close_handle(handle)
                .map_err(HostError::Synchronization)?;
            self.threads.com_hosts.remove(index);
        }
        self.threads
            .com_hosts
            .try_reserve(1)
            .map_err(|_| HostError::Thread(ThreadError32::Allocation))?;
        let id = self.threads.next_id;
        self.ole32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .check_host_apartment(id, sta)?;
        let idle = self.module_gate_at_id(THREAD_RETURN_ID)?;
        let handle = self.create_windows_thread_at_gate(&[0, 0, idle, 0, 0, 0], idle)?;
        self.ole32.as_mut().unwrap().commit_host_apartment(id, sta);
        self.threads
            .threads
            .iter_mut()
            .find(|thread| thread.id == id)
            .expect("canonical thread creation published the host identity")
            .state = ThreadState32::Waiting;
        self.threads.com_hosts.push(ComHost32 {
            thread: id,
            handle,
            sta,
            origin,
        });
        Ok(id)
    }

    fn com_host_candidate(&self, sta: bool, origin: Option<u32>) -> Option<usize> {
        self.threads.com_hosts.iter().position(|host| {
            host.sta == sta
                && host.origin == origin
                && (sta
                    || self.com_apartment_host_idle(host.thread)
                    || self
                        .threads
                        .info(host.thread)
                        .is_ok_and(|info| info.state == ThreadState32::Terminated))
        })
    }

    fn module_gate_at_id(&self, id: u32) -> Result<u32, HostError> {
        self.module_gates()?
            .iter()
            .find(|gate| gate.id == id)
            .map(|gate| gate.entry.0)
            .ok_or(HostError::Thread(ThreadError32::Unsupported))
    }

    pub(in crate::process) fn com_apartment_host(&self, thread: u32) -> bool {
        self.threads
            .com_hosts
            .iter()
            .any(|host| host.thread == thread)
    }

    pub(in crate::process) fn com_apartment_host_idle(&self, thread: u32) -> bool {
        self.com_apartment_host(thread)
            && self.threads.threads.iter().any(|record| {
                record.id == thread
                    && record.state != ThreadState32::Terminated
                    && record.active_invocations.is_empty()
            })
    }

    pub(in crate::process) fn has_active_com_apartment_call(
        &self,
        sender: u32,
        continuation: u64,
    ) -> bool {
        self.threads.threads.iter().any(|thread| {
            thread.active_invocations.iter().any(|invocation| {
                invocation.completion
                    == ThreadInvocationCompletion32::Ole32 {
                        sender,
                        continuation,
                    }
            })
        })
    }

    pub(in crate::process) fn retire_com_sender_invocations(&mut self, sender: u32) {
        self.threads.invocations.retain(|invocation| !matches!(invocation.completion, ThreadInvocationCompletion32::Ole32 {sender: owner,..} if owner == sender));
    }

    pub(in crate::process) fn wake_com_apartment_sender(&mut self, thread: u32) {
        if let Some(thread) = self
            .threads
            .threads
            .iter_mut()
            .find(|record| record.id == thread && record.state != ThreadState32::Terminated)
            && thread.suspend_count == 0
            && thread.state == ThreadState32::Waiting
        {
            thread.state = ThreadState32::Runnable;
        }
        self.threads.schedule_required = Some(self.threads.current_id());
    }

    pub(in crate::process) fn queue_com_apartment_invocation(
        &mut self,
        target: u32,
        procedure: GuestAddress,
        arguments: &[u32],
        sender: u32,
        continuation: u64,
    ) -> Result<(), HostError> {
        let prepared = self.prepare_thread_invocation(
            target,
            procedure,
            arguments,
            ThreadInvocationCompletion32::Ole32 {
                sender,
                continuation,
            },
        )?;
        self.commit_thread_invocation(prepared);
        self.threads.schedule_required = Some(self.threads.current_id());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminated_mta_worker_reaches_the_existing_private_handle_retirement_path() {
        let mut engine = EngineInstance::new(8, 1).unwrap();
        engine.threads.com_hosts.push(ComHost32 {
            thread: 1,
            handle: 17,
            sta: false,
            origin: None,
        });
        engine.threads.threads[0].state = ThreadState32::Terminated;
        assert!(!engine.com_apartment_host_idle(1));
        assert_eq!(engine.com_host_candidate(false, None), Some(0));
        assert_eq!(engine.com_host_candidate(true, None), None);
        assert_eq!(engine.com_host_candidate(false, Some(7)), None);
        assert_eq!(engine.threads.com_hosts[0].handle, 17);
        assert_eq!(engine.threads.com_hosts.len(), 1);
    }
}
