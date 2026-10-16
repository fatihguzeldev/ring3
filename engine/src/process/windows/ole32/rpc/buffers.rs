use super::context::RpcBuffer32;
use crate::{
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, WordWrite32},
    process::{
        EngineInstance, HostError,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{Ole32Runtime32, stream::StreamRuntime32},
        },
    },
    windows::ole32::rpc::RpcMessage32,
};

pub(super) fn read_message(memory: &AddressSpace, address: u32) -> Result<RpcMessage32, HostError> {
    if address == 0 {
        return Err(HostError::InvalidRequest);
    }
    let mut bytes = [0u8; RpcMessage32::SIZE];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(RpcMessage32::decode(&bytes).expect("fixed x86 RPC message extent"))
}
impl Ole32Runtime32 {
    pub(super) fn allocate_rpc_buffer(
        &mut self,
        memory: &mut AddressSpace,
        channel: usize,
        address: u32,
    ) -> Result<(), HostError> {
        let mut message = read_message(memory, address)?;
        memory
            .check_span(GuestAddress(address), RpcMessage32::SIZE, Access::Write)
            .map_err(HostError::Memory)?;
        if self.rpc.buffers.len() == self.rpc.buffers.capacity() {
            return Err(HostError::Infrastructure);
        }
        let pages = (message.length as usize)
            .max(1)
            .div_ceil(PAGE_SIZE as usize);
        let charge = pages
            .checked_mul(PAGE_SIZE as usize)
            .ok_or(HostError::InvalidRequest)?;
        if message.length as usize > self.streams.store.buffer_limit() {
            return Err(HostError::InvalidRequest);
        }
        let prepared = self
            .streams
            .store
            .prepare_external_buffer(charge)
            .map_err(|_| HostError::Infrastructure)?;
        memory
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let scratch = GuestCallbackScratch32::allocate_zeroed_span(
            memory,
            StreamRuntime32::buffer_reservation(),
            charge,
        )
        .map_err(HostError::Memory)?;
        message.buffer = scratch.address.0;
        memory
            .write(GuestAddress(address), &message.encode())
            .expect("message output and both RAM revisions were preflighted");
        let id = self.streams.store.commit_external_buffer(prepared);
        self.rpc.buffers.push(RpcBuffer32 {
            id,
            channel,
            bytes: message.length,
            scratch,
            owner: None,
        });
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn free_rpc_buffer(
        &mut self,
        memory: &mut AddressSpace,
        channel: usize,
        address: u32,
    ) -> Result<(), HostError> {
        self.free_rpc_buffer_for(memory, channel, address, None)
    }

    pub(super) fn free_rpc_buffer_for(
        &mut self,
        memory: &mut AddressSpace,
        channel: usize,
        address: u32,
        owner: Option<u64>,
    ) -> Result<(), HostError> {
        let message = read_message(memory, address)?;
        let index = self
            .rpc
            .packet(channel, message.buffer, message.length, owner)?;
        let buffer = &self.rpc.buffers[index];
        buffer.scratch.check(memory).map_err(HostError::Memory)?;
        let prepared = self
            .streams
            .store
            .prepare_release_buffer(buffer.id)
            .map_err(|_| HostError::Infrastructure)?;
        memory
            .unmap_ranges_with_word_writes(
                &[buffer.scratch.range()],
                &[
                    WordWrite32 {
                        address: GuestAddress(
                            address.checked_add(8).ok_or(HostError::InvalidRequest)?,
                        ),
                        value: 0,
                    },
                    WordWrite32 {
                        address: GuestAddress(
                            address.checked_add(12).ok_or(HostError::InvalidRequest)?,
                        ),
                        value: 0,
                    },
                ],
            )
            .map_err(HostError::Memory)?;
        self.streams.store.commit_release_buffer(prepared);
        self.rpc.buffers.remove(index);
        Ok(())
    }
}

impl EngineInstance {
    pub(super) fn retire_rpc_broker_buffers(&mut self, index: usize) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let mut ranges = Vec::new();
        let mut ids = Vec::new();
        ranges
            .try_reserve_exact(runtime.rpc.buffers.len())
            .map_err(|_| HostError::Infrastructure)?;
        ids.try_reserve_exact(runtime.rpc.buffers.len())
            .map_err(|_| HostError::Infrastructure)?;
        for buffer in runtime
            .rpc
            .buffers
            .iter()
            .filter(|buffer| buffer.channel == index)
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
        if !ranges.is_empty() {
            self.memory
                .as_mut()
                .unwrap()
                .unmap_ranges_with_word_writes(&ranges, &[])
                .map_err(HostError::Memory)?;
        }
        let runtime = self.ole32.as_mut().unwrap();
        runtime.streams.store.commit_release_buffers(release);
        runtime.rpc.buffers.retain(|buffer| buffer.channel != index);
        Ok(())
    }
}
