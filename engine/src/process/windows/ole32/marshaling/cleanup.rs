use super::{Remote32, context::Cleanup32};
use crate::{
    process::{EngineInstance, HostError},
    windows::ole32::ApartmentId32,
};

fn activation_cleanup_ready(remote: &super::RemoteActivation32, thread: u32) -> bool {
    remote.abandoned
        && (remote.execution.thread == thread
            || matches!(
                remote.execution.apartment,
                ApartmentId32::Mta | ApartmentId32::Neutral
            ))
        && remote.cleanup.is_some()
        && remote.remote != Remote32::Queued
}

impl EngineInstance {
    pub(in crate::process) fn com_abandoned_work_ready(&self, thread: u32) -> bool {
        self.com_rundown_work_ready(thread)
            || self.ole32.as_ref().is_some_and(|runtime| {
                runtime
                    .marshaling
                    .activations
                    .iter()
                    .any(|remote| activation_cleanup_ready(remote, thread))
                    || runtime.marshaling.calls.iter().any(|call| {
                        call.abandoned
                            && call.execution.thread == thread
                            && call.cleanup.is_some()
                            && call.remote != Remote32::Queued
                    })
            })
    }

    pub(in crate::process) fn service_abandoned_com_work(&mut self) -> Result<(), HostError> {
        let thread = self.threads.current_id();
        let Some(runtime) = self.ole32.as_ref() else {
            return Ok(());
        };
        if let Some(index) = runtime
            .marshaling
            .activations
            .iter()
            .position(|remote| activation_cleanup_ready(remote, thread))
        {
            let remote = &runtime.marshaling.activations[index];
            let cleanup = remote.cleanup.unwrap();
            let continuation = remote.continuation;
            let pending = runtime
                .activation
                .pending
                .iter()
                .find(|pending| pending.continuation == continuation)
                .ok_or(HostError::Infrastructure)?;
            let sender = pending.thread;
            if cleanup.next == cleanup.count {
                pending
                    .scratch
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                pending
                    .scratch
                    .release(self.memory.as_mut().unwrap())
                    .map_err(HostError::Memory)?;
                let runtime = self.ole32.as_mut().unwrap();
                runtime
                    .activation
                    .pending
                    .retain(|pending| pending.continuation != continuation);
                runtime.marshaling.activations.remove(index);
            } else {
                let mut execution = remote.execution;
                if matches!(
                    execution.apartment,
                    ApartmentId32::Mta | ApartmentId32::Neutral
                ) {
                    execution.thread = self.ensure_com_apartment_host(false)?;
                }
                self.queue_com_cleanup(execution.thread, sender, continuation, cleanup)?;
                let remote = &mut self.ole32.as_mut().unwrap().marshaling.activations[index];
                remote.execution = execution;
                remote.remote = Remote32::Queued;
            }
            return Ok(());
        }
        let Some(index) = runtime.marshaling.calls.iter().position(|call| {
            call.abandoned
                && call.execution.thread == thread
                && call.cleanup.is_some()
                && call.remote != Remote32::Queued
        }) else {
            return self.service_com_rundown();
        };
        let call = &runtime.marshaling.calls[index];
        let cleanup = call.cleanup.unwrap();
        let (continuation, sender) = (call.continuation, call.thread);
        let (method, proxy) = (call.method, call.proxy);
        if cleanup.next == cleanup.count {
            call.scratch
                .check(self.memory()?)
                .map_err(HostError::Memory)?;
            call.scratch
                .release(self.memory.as_mut().unwrap())
                .map_err(HostError::Memory)?;
            let runtime = self.ole32.as_mut().unwrap();
            if method.0 == 2 {
                runtime.marshaling.proxies[proxy].as_mut().unwrap().owned = [0; 2];
            }
            runtime.marshaling.calls.remove(index);
        } else {
            self.queue_com_cleanup(thread, sender, continuation, cleanup)?;
            self.ole32.as_mut().unwrap().marshaling.calls[index].remote = Remote32::Queued;
        }
        Ok(())
    }

    fn queue_com_cleanup(
        &mut self,
        target: u32,
        sender: u32,
        continuation: u64,
        cleanup: Cleanup32,
    ) -> Result<(), HostError> {
        let pointer = cleanup.pointers[cleanup.next];
        let procedure = self.com_guest_method(pointer, 2)?;
        self.queue_com_apartment_invocation(target, procedure, &[pointer], sender, continuation)
    }
}
