use super::{super::marshaling::Remote32, context::RpcPhase32};
use crate::{
    process::{EngineInstance, HostError},
    windows::ole32::ApartmentId32,
};
pub(in crate::process::windows::ole32) struct PreparedRpcThreadExit32 {
    calls: Vec<(u64, bool, bool, bool)>,
    sources: Vec<usize>,
    clients: Vec<usize>,
    releases: Vec<crate::memory::GuestAddress>,
}
impl PreparedRpcThreadExit32 {
    pub(in crate::process::windows::ole32) fn releases(&self) -> &[crate::memory::GuestAddress] {
        &self.releases
    }
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn prepare_rpc_thread_exit(
        &self,
        thread: u32,
    ) -> Result<PreparedRpcThreadExit32, HostError> {
        let mut result = PreparedRpcThreadExit32 {
            calls: Vec::new(),
            sources: Vec::new(),
            clients: Vec::new(),
            releases: Vec::new(),
        };
        let Some(runtime) = &self.ole32 else {
            return Ok(result);
        };
        result
            .calls
            .try_reserve_exact(runtime.rpc.calls.len())
            .map_err(|_| HostError::Infrastructure)?;
        result
            .sources
            .try_reserve_exact(runtime.rpc.brokers.len())
            .map_err(|_| HostError::Infrastructure)?;
        result
            .clients
            .try_reserve_exact(runtime.rpc.brokers.len())
            .map_err(|_| HostError::Infrastructure)?;
        for call in &runtime.rpc.calls {
            let sender = call.sender == thread;
            let target = call.execution.thread == thread
                && (call.remote == Remote32::Queued
                    || call.execution.apartment == ApartmentId32::Sta(thread));
            if sender || target {
                if let Some(scratch) = call.scratch {
                    scratch.check(self.memory()?).map_err(HostError::Memory)?;
                }
                for buffer in runtime
                    .rpc
                    .buffers
                    .iter()
                    .filter(|buffer| buffer.owner == Some(call.continuation))
                {
                    buffer
                        .scratch
                        .check(self.memory()?)
                        .map_err(HostError::Memory)?;
                }
                result.calls.push((
                    call.continuation,
                    sender,
                    target,
                    self.has_active_com_apartment_call(call.sender, call.continuation) && !target,
                ));
            }
        }
        for (index, broker) in runtime
            .rpc
            .brokers
            .iter()
            .enumerate()
            .filter_map(|(index, broker)| broker.as_ref().map(|broker| (index, broker)))
        {
            if broker.source.apartment == ApartmentId32::Sta(thread) {
                result.sources.push(index)
            }
            if broker.client.apartment == ApartmentId32::Sta(thread) {
                result.clients.push(index);
                if broker.proxy_channel_references != 0 {
                    let pointer = broker.channel.ok_or(HostError::Infrastructure)?;
                    runtime
                        .com
                        .preflight(self.memory()?, pointer)
                        .map_err(super::calls::com_error)?;
                    result
                        .releases
                        .try_reserve_exact(broker.proxy_channel_references as usize)
                        .map_err(|_| HostError::Infrastructure)?;
                    result.releases.extend(core::iter::repeat_n(
                        pointer,
                        broker.proxy_channel_references as usize,
                    ));
                }
            }
        }
        Ok(result)
    }
    pub(in crate::process::windows::ole32) fn commit_rpc_thread_exit(
        &mut self,
        prepared: PreparedRpcThreadExit32,
    ) {
        let Some(runtime) = &mut self.ole32 else {
            return;
        };
        for index in prepared.sources {
            let broker = runtime.rpc.brokers[index].as_mut().unwrap();
            broker.stub = 0;
            broker.source_factory = 0;
            broker.source_object = 0;
            broker.owns_source = false;
            broker.connected = false;
            if let Some(rundown) = broker.rundown.as_mut()
                && rundown.remote == Remote32::Queued
                && rundown.execution.apartment == broker.source.apartment
            {
                rundown.remote = Remote32::Idle;
            }
        }
        for index in prepared.clients {
            let broker = runtime.rpc.brokers[index].as_mut().unwrap();
            broker.proxy = 0;
            broker.client_factory = 0;
            broker.interface = 0;
            broker.connected = false;
            broker.proxy_channel_references = 0;
            if let Some(rundown) = broker.rundown.as_mut()
                && rundown.remote == Remote32::Queued
                && rundown.execution.apartment == broker.client.apartment
            {
                rundown.remote = Remote32::Idle;
            }
        }
        let mut waking = [0u32; 64];
        let mut count = 0;
        for (id, sender, target, active) in prepared.calls {
            let call = runtime
                .rpc
                .calls
                .iter_mut()
                .find(|call| call.continuation == id)
                .unwrap();
            call.abandoned |= sender;
            if target {
                call.cancelled = true;
                call.remote = Remote32::Ready(0x80010108);
                if !sender {
                    waking[count] = call.sender;
                    count += 1;
                }
            } else if sender && !active && call.remote == Remote32::Queued {
                call.remote = Remote32::Idle;
                call.cancelled = true;
            }
        }
        for thread in &waking[..count] {
            self.wake_com_apartment_sender(*thread);
        }
    }
    pub(in crate::process::windows::ole32) fn abandoned_rpc_work_ready(&self) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.rpc.calls.iter().any(|call| {
                call.abandoned
                    && call.remote != Remote32::Queued
                    && call.phase != RpcPhase32::Transfer
            })
        })
    }
    pub(in crate::process::windows::ole32) fn service_abandoned_rpc_work(
        &mut self,
    ) -> Result<bool, HostError> {
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime.rpc.calls.iter().position(|call| {
                call.abandoned
                    && call.remote != Remote32::Queued
                    && call.phase != RpcPhase32::Transfer
            })
        }) else {
            return Ok(false);
        };
        let call = &self.ole32.as_ref().unwrap().rpc.calls[index];
        match call.phase {
            RpcPhase32::Cleanup(_) | RpcPhase32::ReleaseQuery => self.advance_rpc_call(index)?,
            RpcPhase32::Query(_) => {
                let pointer = if !call.cancelled
                    && matches!(call.remote,Remote32::Ready(value) if value&0x80000000==0)
                {
                    self.ole32_read_word(call.scratch.unwrap().address.0 + 16)?
                } else {
                    0
                };
                let call = &mut self.ole32.as_mut().unwrap().rpc.calls[index];
                call.returned = pointer;
                call.phase = if pointer == 0 {
                    RpcPhase32::Retire
                } else {
                    RpcPhase32::ReleaseQuery
                };
                call.remote = Remote32::Idle;
                self.advance_rpc_call(index)?;
            }
            _ => {
                self.ole32.as_mut().unwrap().rpc.calls[index].phase = RpcPhase32::Retire;
                self.advance_rpc_call(index)?;
            }
        }
        Ok(true)
    }
    pub(super) fn retire_abandoned_rpc_call(&mut self, index: usize) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.rpc.calls[index];
        let id = call.continuation;
        let mut ranges = Vec::new();
        let mut ids = Vec::new();
        ranges
            .try_reserve_exact(runtime.rpc.buffers.len() + 1)
            .map_err(|_| HostError::Infrastructure)?;
        ids.try_reserve_exact(runtime.rpc.buffers.len())
            .map_err(|_| HostError::Infrastructure)?;
        if let Some(scratch) = call.scratch {
            scratch.check(self.memory()?).map_err(HostError::Memory)?;
            ranges.push(scratch.range());
        }
        for buffer in runtime
            .rpc
            .buffers
            .iter()
            .filter(|buffer| buffer.owner == Some(id))
        {
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
        let unhold = call.held_channel.map(|channel| [channel]);
        let lifetime = runtime
            .com
            .prepare_lifetime_changes_with_unmaps(
                self.memory()?,
                &[],
                &[],
                &[],
                unhold.as_ref().map_or(&[], |pointers| pointers.as_slice()),
                &[],
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
        runtime
            .rpc
            .buffers
            .retain(|buffer| buffer.owner != Some(id));
        runtime.rpc.calls.remove(index);
        Ok(())
    }
}
