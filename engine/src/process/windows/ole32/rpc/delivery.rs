use super::{buffers::read_message, context::RpcPhase32 as P};
use crate::{
    memory::{GuestAddress, WordWrite32},
    process::{EngineInstance, HostError, windows::ole32::marshaling::Remote32},
    windows::ole32::ApartmentId32,
};
impl EngineInstance {
    pub(super) fn advance_rpc_call(&mut self, index: usize) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.rpc.calls[index];
        if call.remote == Remote32::Queued {
            return Ok(());
        }
        if let Some(scratch) = call.scratch {
            scratch.check(self.memory()?).map_err(HostError::Memory)?;
        }
        if call.phase == P::Retire {
            return self.retire_abandoned_rpc_call(index);
        }

        if call.phase == P::Transfer {
            return self.finish_rpc_query_transfer(call.continuation);
        }
        if let Remote32::Ready(result) = call.remote {
            match call.phase {
                P::Invoke => return self.finish_rpc_invoke(index, result),
                P::Query(requested) => {
                    return if result & 0x80000000 == 0 {
                        self.start_rpc_query_transfer(index, requested, result)
                    } else {
                        self.finish_rpc_query(index, result, 0)
                    };
                }
                P::Cleanup(step) => {
                    let runtime = self.ole32.as_mut().unwrap();
                    let broker = runtime.rpc.brokers[runtime.rpc.calls[index].broker]
                        .as_mut()
                        .unwrap();
                    match step {
                        1 => broker.proxy = 0,
                        3 => broker.stub = 0,
                        4 => {
                            broker.source_object = 0;
                            broker.owns_source = false
                        }
                        _ => {}
                    }
                    let call = &mut runtime.rpc.calls[index];
                    call.phase = P::Cleanup(step + 1);
                    call.remote = Remote32::Idle;
                    return self.advance_rpc_call(index);
                }
                P::ReleaseQuery => {
                    let call = &mut self.ole32.as_mut().unwrap().rpc.calls[index];
                    call.returned = 0;
                    call.phase = P::Retire;
                    call.remote = Remote32::Idle;
                    return self.retire_abandoned_rpc_call(index);
                }
                P::Retire => return self.retire_abandoned_rpc_call(index),
                P::Transfer => unreachable!(),
            }
        }
        let broker = *runtime.rpc.brokers[call.broker]
            .as_ref()
            .ok_or(HostError::Infrastructure)?;
        let (pointer, slot, args, count, mut execution) = match call.phase {
            P::Invoke => (
                broker.stub,
                5,
                [
                    broker.stub,
                    call.scratch.unwrap().address.0,
                    broker.channel.unwrap().0,
                ],
                3,
                broker.source,
            ),
            P::Query(_) => (
                broker.source_object,
                0,
                [
                    broker.source_object,
                    call.scratch.unwrap().address.0,
                    call.scratch.unwrap().address.0 + 16,
                ],
                3,
                broker.source,
            ),
            P::Cleanup(step) => {
                let (pointer, slot, execution) = match step {
                    0 => (broker.proxy, 4, broker.client),
                    1 => (broker.proxy, 2, broker.client),
                    2 => (broker.stub, 4, broker.source),
                    3 => (broker.stub, 2, broker.source),
                    4 => (
                        if broker.owns_source {
                            broker.source_object
                        } else {
                            0
                        },
                        2,
                        broker.source,
                    ),
                    _ => return self.finish_rpc_cleanup(index),
                };
                if pointer == 0 {
                    self.ole32.as_mut().unwrap().rpc.calls[index].phase = P::Cleanup(step + 1);
                    return self.advance_rpc_call(index);
                }
                (pointer, slot, [pointer, 0, 0], 1, execution)
            }
            P::ReleaseQuery => (call.returned, 2, [call.returned, 0, 0], 1, broker.source),
            P::Retire => return self.retire_abandoned_rpc_call(index),
            P::Transfer => unreachable!(),
        };
        let (sender, continuation) = (call.sender, call.continuation);
        if matches!(
            execution.apartment,
            ApartmentId32::Mta | ApartmentId32::Neutral
        ) && (execution.thread != sender || call.abandoned)
        {
            execution.thread = self.ensure_com_apartment_host(false)?;
        }
        let procedure = self.com_guest_method(pointer, slot)?;
        self.queue_com_apartment_invocation(
            execution.thread,
            procedure,
            &args[..count],
            sender,
            continuation,
        )?;
        let call = &mut self.ole32.as_mut().unwrap().rpc.calls[index];
        call.execution = execution;
        call.remote = Remote32::Queued;
        Ok(())
    }
    fn finish_rpc_invoke(&mut self, index: usize, result: u32) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.rpc.calls[index];
        let prepared = Self::prepare_completed_call(call.frame.complete(result))?;
        let scratch = call.scratch.unwrap();
        scratch.check(self.memory()?).map_err(HostError::Memory)?;
        let mut response = if call.cancelled {
            call.request.ok_or(HostError::Infrastructure)?
        } else {
            read_message(self.memory()?, scratch.address.0)?
        };
        let response_index = runtime.rpc.packet(
            call.broker,
            response.buffer,
            response.length,
            Some(call.continuation),
        );
        let original_request = response_index.as_ref().is_ok_and(|index| {
            Some(runtime.rpc.buffers[*index].id) == call.request_lease
                && runtime.rpc.buffers[*index].owner == Some(call.continuation)
        });
        if call.cancelled && !original_request {
            // The active stub may have freed the original request while
            // constructing its response. A disconnected caller receives an
            // empty packet, never a pointer to an already-retired request.
            response.buffer = 0;
            response.length = 0;
        } else {
            runtime.rpc.buffers[response_index?]
                .scratch
                .check(self.memory()?)
                .map_err(HostError::Memory)?;
        }
        let continuation = call.continuation;
        let mut ranges = Vec::new();
        let mut ids = Vec::new();
        ranges
            .try_reserve_exact(runtime.rpc.buffers.len() + 1)
            .map_err(|_| HostError::Infrastructure)?;
        ids.try_reserve_exact(runtime.rpc.buffers.len())
            .map_err(|_| HostError::Infrastructure)?;
        ranges.push(scratch.range());
        for buffer in runtime.rpc.buffers.iter().filter(|buffer| {
            buffer.owner == Some(continuation) && buffer.scratch.address.0 != response.buffer
        }) {
            buffer
                .scratch
                .check(self.memory()?)
                .map_err(HostError::Memory)?;
            ranges.push(buffer.scratch.range());
            ids.push(buffer.id);
        }
        let release = runtime
            .streams
            .store
            .prepare_release_buffers(&ids)
            .map_err(|_| HostError::Infrastructure)?;
        ranges.sort_unstable_by_key(|range| range.first);
        let bytes = response.encode();
        let mut words = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; 12];
        for (slot, word) in bytes.chunks_exact(4).enumerate() {
            words[slot] = WordWrite32 {
                address: GuestAddress(
                    call.message
                        .checked_add(slot as u32 * 4)
                        .ok_or(HostError::InvalidRequest)?,
                ),
                value: u32::from_le_bytes(word.try_into().unwrap()),
            };
        }
        let mut word_count = 11;
        if call.status != 0 && result & 0x80000000 == 0 {
            words[11] = WordWrite32 {
                address: GuestAddress(call.status),
                value: 0,
            };
            word_count = 12;
        }
        words[..word_count].sort_unstable_by_key(|word| word.address.0);
        let unhold = call.held_channel.map(|channel| [channel]);
        let lifetime = runtime
            .com
            .prepare_lifetime_changes_with_unmaps(
                self.memory()?,
                &[],
                &[],
                &[],
                unhold.as_ref().map_or(&[], |pointers| pointers.as_slice()),
                &words[..word_count],
                &ranges,
            )
            .map_err(super::calls::com_error)?;
        let runtime = self.ole32.as_mut().unwrap();
        let destroyed = runtime
            .com
            .commit_lifetime_changes(self.memory.as_mut().unwrap(), lifetime)
            .map_err(super::calls::com_error)?;
        runtime.rpc.record_destroyed_identities(&destroyed);
        runtime.streams.store.commit_release_buffers(release);
        runtime.rpc.buffers.retain(|buffer| {
            buffer.owner != Some(continuation) || buffer.scratch.address.0 == response.buffer
        });
        if response.buffer != 0 {
            let response = runtime
                .rpc
                .buffer(
                    runtime.rpc.calls[index].broker,
                    response.buffer,
                    response.length,
                )
                .expect("validated response lease retained");
            runtime.rpc.buffers[response].owner = None;
        }
        runtime.rpc.calls.remove(index);
        self.publish_prepared_call(prepared);
        Ok(())
    }
    fn finish_rpc_cleanup(&mut self, index: usize) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().rpc.calls[index];
        let broker = call.broker;
        let prepared = (!call.abandoned)
            .then(|| Self::prepare_completed_call(call.frame.complete(0)))
            .transpose()?;
        self.retire_rpc_broker_buffers(broker)?;
        let runtime = self.ole32.as_mut().unwrap();
        runtime.rpc.calls.remove(index);
        if !runtime.rpc.brokers[broker].as_ref().unwrap().channel_live {
            runtime.rpc.brokers[broker] = None;
        }
        if let Some(prepared) = prepared {
            self.publish_prepared_call(prepared);
        }
        Ok(())
    }
}
