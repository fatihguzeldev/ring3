use super::context::TransferPhase32 as P;
use crate::{
    process::{EngineInstance, HostError, windows::ole32::marshaling::Remote32},
    windows::ole32::Ole32Api32,
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn advance_com_transfer(
        &mut self,
        index: usize,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        self.validate_com_transfer_identity(call)?;
        if call.phase == P::Complete || call.remote == Remote32::Queued {
            return Ok(());
        }
        self.transfer_scratch_check(index)?;
        if let Remote32::Ready(result) = call.remote {
            return self.accept_com_transfer_result(index, result);
        }
        if let P::ProxyStub(broker) = call.phase {
            return if call.abandoned || call.cancelled {
                self.fail_registered_ps_transfer(index, broker, 0x80010108)
            } else {
                self.advance_registered_ps_transfer(index, broker)
            };
        }
        if (call.abandoned || call.cancelled)
            && !matches!(call.phase, P::Release(_) | P::DiscardPacket)
        {
            return self.fail_com_transfer(index, 0x8001_0108);
        }
        if let P::Release(next) = call.phase {
            return self.advance_com_transfer_release(index, next);
        }
        let scratch = call.scratch().address.0;
        let stream = call.stream.map_or(0, |pointer| pointer.0);
        let source = call.pointer(0);
        let marshaler = call.pointer(1);
        let (object, slot, args, count, mut execution) = match call.phase {
            P::QueryMarshal => (
                source,
                0,
                [source, scratch + 16, scratch + 48, 0, 0, 0, 0],
                3,
                call.source,
            ),
            P::Class => (
                marshaler,
                3,
                [marshaler, scratch, source, 3, 0, 0, scratch + 32],
                7,
                call.source,
            ),
            P::Size => (
                marshaler,
                4,
                [marshaler, scratch, source, 3, 0, 0, scratch + 52],
                7,
                call.source,
            ),
            P::Marshal => (
                marshaler,
                5,
                [marshaler, stream, scratch, source, 3, 0, 0],
                7,
                call.source,
            ),
            P::Activate => (
                0,
                0,
                [scratch + 32, 0, 1, scratch + 16, scratch + 56, 0, 0],
                5,
                call.client,
            ),
            P::Unmarshal => {
                let pointer = call.pointer(2);
                (
                    pointer,
                    6,
                    [pointer, stream, scratch, scratch + 60, 0, 0, 0],
                    4,
                    call.client,
                )
            }
            P::DiscardPacket => (
                marshaler,
                7,
                [marshaler, stream, 0, 0, 0, 0, 0],
                2,
                call.source,
            ),
            P::Release(_) | P::ProxyStub(_) | P::Complete => unreachable!(),
        };
        let (sender, continuation) = (call.sender, call.continuation);
        let phase = call.phase;
        if (execution.apartment == crate::windows::ole32::ApartmentId32::Mta
            && (execution.thread != sender || call.abandoned))
            || (call.abandoned
                && execution.apartment == crate::windows::ole32::ApartmentId32::Neutral)
        {
            execution.thread = self.ensure_com_apartment_host(false)?;
        }
        let procedure = if phase == P::Activate {
            let gates = self.module_gates()?;
            gates
                .iter()
                .find(|gate| gate.id == Ole32Api32::CoCreateInstance.id())
                .ok_or(HostError::Infrastructure)?
                .entry
        } else {
            self.com_guest_method(object, slot)?
        };
        self.queue_com_apartment_invocation(
            execution.thread,
            procedure,
            &args[..count],
            sender,
            continuation,
        )?;
        let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[index];
        call.execution = execution;
        call.remote = Remote32::Queued;
        Ok(())
    }
    pub(super) fn rewind_transfer_stream(&mut self, index: usize) -> Result<(), HostError> {
        let pointer = self.ole32.as_ref().unwrap().marshaling.transfers[index]
            .stream
            .ok_or(HostError::Infrastructure)?;
        let runtime = self.ole32.as_ref().unwrap();
        let id = runtime
            .stream_id(self.memory()?, pointer.0)
            .map_err(|_| HostError::Infrastructure)?;
        let prepared = runtime
            .streams
            .store
            .prepare_seek(id, 0, 0)
            .map_err(|_| HostError::Infrastructure)?;
        self.ole32
            .as_mut()
            .unwrap()
            .streams
            .store
            .commit_seek(prepared);
        Ok(())
    }
}
