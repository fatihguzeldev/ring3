use super::context::{CHUNK, CopyPhase32};
use crate::{
    memory::{GuestAddress, WordWrite32},
    process::{EngineInstance, HostError, windows::ole32::marshaling::Remote32},
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn check_stream_copy(
        &self,
        index: usize,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.streams.copies[index];
        call.scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        call.witness
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        if self.ole32_read_word(call.destination)? != call.table
            || self.com_guest_method(call.destination, 4)? != call.procedure
        {
            return Err(HostError::InvalidRequest);
        }
        let source = runtime.stream_index(self.memory()?, call.source.0)?;
        if runtime.streams.entries[source].unwrap().id != call.id {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }
    pub(in crate::process::windows::ole32) fn advance_stream_copy(
        &mut self,
        index: usize,
    ) -> Result<(), HostError> {
        self.check_stream_copy(index)?;
        let call = &self.ole32.as_ref().unwrap().streams.copies[index];
        if call.remote == Remote32::Queued {
            return Ok(());
        }
        if let Remote32::Ready(result) = call.remote {
            let written = self.ole32_read_word(call.scratch.address.0 + CHUNK as u32)?;
            if written > call.chunk {
                return Err(HostError::InvalidRequest);
            }
            let call = &mut self.ole32.as_mut().unwrap().streams.copies[index];
            call.written += u64::from(written);
            call.remote = Remote32::Idle;
            call.phase = if result & 0x80000000 != 0 {
                CopyPhase32::Finish(result)
            } else if written != call.chunk {
                CopyPhase32::Finish(0x8003_0070)
            } else {
                CopyPhase32::Read
            };
        }
        let call = &self.ole32.as_ref().unwrap().streams.copies[index];
        if let CopyPhase32::Finish(result) = call.phase {
            return self.finish_stream_copy(index, result);
        }
        if call.remaining == 0 {
            return self.finish_stream_copy(index, 0);
        }
        let read = self
            .ole32
            .as_ref()
            .unwrap()
            .streams
            .store
            .prepare_read(call.id, call.remaining.min(CHUNK as u64) as usize)
            .map_err(|_| HostError::Infrastructure)?;
        if read.length() == 0 {
            return self.finish_stream_copy(index, 0);
        }
        let count = read.length();
        let scratch = call.scratch.address;
        let invocation = self.prepare_thread_invocation(
            call.execution.thread,
            call.procedure,
            &[
                call.destination,
                scratch.0,
                count as u32,
                scratch.0 + CHUNK as u32,
            ],
            crate::process::windows::threads::ThreadInvocationCompletion32::Ole32 {
                sender: call.execution.thread,
                continuation: call.continuation,
            },
        )?;
        let bytes = self
            .ole32
            .as_ref()
            .unwrap()
            .streams
            .store
            .read_bytes(&read)
            .map_err(|_| HostError::Infrastructure)?;
        self.memory
            .as_mut()
            .unwrap()
            .write_spans(&[
                (scratch, bytes),
                (GuestAddress(scratch.0 + CHUNK as u32), &0u32.to_le_bytes()),
            ])
            .map_err(HostError::Memory)?;
        let runtime = self.ole32.as_mut().unwrap();
        runtime.streams.store.commit_read(read);
        let call = &mut runtime.streams.copies[index];
        call.read += count as u64;
        call.remaining -= count as u64;
        call.chunk = count as u32;
        call.phase = CopyPhase32::Write;
        call.remote = Remote32::Queued;
        let thread = call.execution.thread;
        self.commit_thread_invocation(invocation);
        self.wake_com_apartment_sender(thread);
        Ok(())
    }
    fn finish_stream_copy(&mut self, index: usize, result: u32) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.streams.copies[index];
        let prepared = Self::prepare_completed_call(call.frame.complete(result))?;
        let args = call.frame.arguments();
        let mut words = Vec::new();
        words
            .try_reserve_exact(4)
            .map_err(|_| HostError::Infrastructure)?;
        for (address, count) in [(args[4], call.read), (args[5], call.written)] {
            if address != 0 {
                for offset in [0, 4] {
                    let word = WordWrite32 {
                        address: GuestAddress(address + offset),
                        value: (count >> (offset * 8)) as u32,
                    };
                    if let Some(existing) = words
                        .iter_mut()
                        .find(|existing: &&mut WordWrite32| existing.address == word.address)
                    {
                        *existing = word;
                    } else {
                        words.push(word);
                    }
                }
            }
        }
        words.sort_unstable_by_key(|word| word.address.0);
        let lifetime = runtime
            .com
            .prepare_lifetime_changes_with_unmaps(
                self.memory()?,
                &[],
                &[],
                &[],
                &[call.source],
                &words,
                &[call.scratch.range()],
            )
            .map_err(super::super::super::rpc::com_reference_error)?;
        let streams = runtime.prepare_destroyed_streams(lifetime.destroyed())?;
        let runtime = self.ole32.as_mut().unwrap();
        runtime
            .com
            .commit_lifetime_changes(self.memory.as_mut().unwrap(), lifetime)
            .map_err(super::super::super::rpc::com_reference_error)?;
        runtime.commit_destroyed_streams(streams);
        runtime.streams.copies.remove(index);
        self.publish_prepared_call(prepared);
        Ok(())
    }
}
