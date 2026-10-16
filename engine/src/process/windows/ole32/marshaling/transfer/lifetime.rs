use super::context::TransferPhase32 as P;
use crate::process::{EngineInstance, HostError, windows::ole32::marshaling::Remote32};
impl EngineInstance {
    pub(super) fn advance_com_transfer_release(
        &mut self,
        index: usize,
        mut next: usize,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        let limit = if call.output != 0 && !call.abandoned {
            3
        } else {
            4
        };
        while next < limit && call.pointer(next) == 0 {
            next += 1;
        }
        if next == limit {
            let scratch = call.scratch;
            if let Some(scratch) = scratch {
                scratch.check(self.memory()?).map_err(HostError::Memory)?;
            }
            let stream = call.stream;
            let stream_index = stream
                .map(|pointer| {
                    self.ole32
                        .as_ref()
                        .unwrap()
                        .stream_index(self.memory()?, pointer.0)
                })
                .transpose()?;
            self.memory()?
                .ensure_transaction_versions(
                    u64::from(scratch.is_some()) + u64::from(stream_index.is_some()),
                )
                .map_err(HostError::Memory)?;
            if let Some(stream) = stream_index {
                self.ole32
                    .as_mut()
                    .unwrap()
                    .release_stream(self.memory.as_mut().unwrap(), stream)
                    .map_err(|_| HostError::Infrastructure)?;
            }
            if let Some(scratch) = scratch {
                scratch
                    .release(self.memory.as_mut().unwrap())
                    .expect("transfer scratch and all RAM version increments were preflighted");
            }
            let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
            call.scratch = None;
            call.stream = None;
            call.remote = Remote32::Idle;
            call.phase = P::Complete;
            return Ok(());
        }
        let mut reference = call.references[next];
        let (sender, continuation) = (call.sender, call.continuation);
        if reference.execution.apartment == crate::windows::ole32::ApartmentId32::Mta
            || reference.execution.apartment == crate::windows::ole32::ApartmentId32::Neutral
        {
            reference.execution.thread = self.ensure_com_apartment_host(false)?;
        }
        let procedure = self.com_guest_method(reference.pointer, 2)?;
        self.queue_com_apartment_invocation(
            reference.execution.thread,
            procedure,
            &[reference.pointer],
            sender,
            continuation,
        )?;
        let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
        call.execution = reference.execution;
        call.phase = P::Release(next);
        call.remote = Remote32::Queued;
        Ok(())
    }
}
