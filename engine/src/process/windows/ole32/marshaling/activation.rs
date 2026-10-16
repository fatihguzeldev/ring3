use super::{ExecutionApartment32, Remote32};
use crate::{
    process::{
        EngineInstance, HostError,
        windows::{callbacks::WindowsCallback32, ole32::Ole32CallbackOutcome32},
    },
    windows::{
        com::ServerThreadingModel32,
        ole32::{ApartmentId32, ApartmentTarget32},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn select_com_execution_apartment(
        &mut self,
        thread: u32,
        model: ServerThreadingModel32,
    ) -> Result<ExecutionApartment32, HostError> {
        let target = self
            .ole32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .apartments
            .server_target(thread, model)
            .map_err(|_| HostError::Infrastructure)?;
        let (apartment, target) = match target {
            ApartmentTarget32::Current(apartment) => (apartment, thread),
            ApartmentTarget32::Sta(thread) => (ApartmentId32::Sta(thread), thread),
            ApartmentTarget32::HostSta => {
                let host = self.ensure_com_apartment_host(true)?;
                (ApartmentId32::Sta(host), host)
            }
            ApartmentTarget32::HostMta => {
                (ApartmentId32::Mta, self.ensure_com_apartment_host(false)?)
            }
        };
        Ok(ExecutionApartment32 {
            apartment,
            thread: target,
        })
    }

    pub(in crate::process::windows::ole32) fn remote_com_activation(
        &self,
        continuation: u64,
    ) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .marshaling
                .activations
                .iter()
                .any(|remote| remote.continuation == continuation)
        })
    }

    pub(in crate::process::windows::ole32) fn remote_com_activation_ready(
        &self,
        continuation: u64,
    ) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.marshaling.activations.iter().any(|remote| {
                remote.continuation == continuation
                    && (remote.next.is_some() || matches!(remote.remote, Remote32::Ready(_)))
            })
        })
    }

    pub(in crate::process::windows::ole32) fn dispatch_registered_com_callback(
        &mut self,
        slot: usize,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        let continuation = self.ole32.as_ref().unwrap().activation.pending[slot].continuation;
        let Some(index) = self
            .ole32
            .as_ref()
            .unwrap()
            .marshaling
            .activations
            .iter()
            .position(|remote| remote.continuation == continuation)
        else {
            return self.park_ole32_callback(request);
        };
        self.ole32.as_mut().unwrap().marshaling.activations[index].next = Some(request);
        self.queue_remote_com_activation(slot, index)
    }

    fn queue_remote_com_activation(&mut self, slot: usize, index: usize) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let pending = &runtime.activation.pending[slot];
        let remote = &runtime.marshaling.activations[index];
        let request = remote.next.ok_or(HostError::InvalidRequest)?;
        let (mut execution, sender) = (remote.execution, pending.thread);
        if execution.apartment == ApartmentId32::Mta {
            execution.thread = self.ensure_com_apartment_host(false)?;
        }
        self.queue_com_apartment_invocation(
            execution.thread,
            request.procedure,
            &request.arguments[..request.count as usize],
            sender,
            request.continuation,
        )?;
        let remote = &mut self.ole32.as_mut().unwrap().marshaling.activations[index];
        remote.execution = execution;
        remote.remote = Remote32::Queued;
        remote.next = None;
        Ok(())
    }

    pub(in crate::process::windows::ole32) fn advance_remote_com_activation(
        &mut self,
        slot: usize,
    ) -> Result<bool, HostError> {
        let pending = &self.ole32.as_ref().unwrap().activation.pending[slot];
        let (continuation, frame) = (pending.continuation, pending.frame);
        let Some(index) = self
            .ole32
            .as_ref()
            .unwrap()
            .marshaling
            .activations
            .iter()
            .position(|remote| remote.continuation == continuation)
        else {
            return Ok(false);
        };
        if self.ole32.as_ref().unwrap().marshaling.activations[index]
            .next
            .is_some()
        {
            self.queue_remote_com_activation(slot, index)?;
            return Ok(true);
        }
        let Remote32::Ready(result) =
            self.ole32.as_ref().unwrap().marshaling.activations[index].remote
        else {
            return Ok(false);
        };
        match self.resume_ole32_callback(continuation, result)? {
            Ole32CallbackOutcome32::Complete(result) => {
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(result))?);
            }
            Ole32CallbackOutcome32::Callback(request) => {
                self.dispatch_registered_com_callback(slot, request)?
            }
            Ole32CallbackOutcome32::LoaderPending => return Err(HostError::Infrastructure),
        }
        Ok(true)
    }
}
