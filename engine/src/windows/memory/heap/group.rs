use super::*;

pub(crate) struct PreparedHeapAllocations32 {
    handle: u32,
    original: Vec<Block32>,
    candidate: Vec<Block32>,
    addresses: Vec<u32>,
    sizes: Vec<u32>,
    extents: Vec<(u32, u32)>,
    flags: u32,
    maximum: u32,
    small: bool,
    identity: u64,
    version: u64,
    previous_allocation: u64,
    next_allocation: u64,
}
impl PreparedHeapAllocations32 {
    pub(crate) fn addresses(&self) -> &[u32] {
        &self.addresses
    }
}
impl HeapManager32 {
    pub(crate) fn prepare_initialized_allocations(
        &self,
        memory: &AddressSpace,
        handle: u32,
        sizes: &[u32],
        retire: &[u32],
    ) -> Result<PreparedHeapAllocations32, MemoryError32> {
        let heap = self.index(handle)?;
        let source = &self.heaps[heap];
        let next_allocation = self
            .next_allocation
            .checked_add(sizes.len() as u64)
            .ok_or(MemoryError32::Capacity)?;
        if sizes.len() > self.limits.blocks || retire.len() > source.blocks.len() {
            return Err(MemoryError32::Capacity);
        }
        let reserve = source
            .blocks
            .len()
            .checked_add(sizes.len())
            .ok_or(MemoryError32::Capacity)?;
        let mut original = Vec::new();
        original
            .try_reserve_exact(source.blocks.len())
            .map_err(|_| MemoryError32::Allocation)?;
        original.extend_from_slice(&source.blocks);
        let mut candidate = Vec::new();
        candidate
            .try_reserve_exact(reserve)
            .map_err(|_| MemoryError32::Allocation)?;
        candidate.extend_from_slice(&source.blocks);
        for (position, &pointer) in retire.iter().enumerate() {
            if retire[..position].contains(&pointer) {
                return Err(MemoryError32::InvalidParameter);
            }
            let block = self.block(heap, pointer)?;
            let old = source.blocks[block];
            memory.check_span(
                GuestAddress(pointer),
                old.size.unwrap() as usize,
                crate::memory::Access::Write,
            )?;
            candidate[block].size = None;
            candidate[block].allocation = 0;
        }
        coalesce_blocks(&mut candidate);
        let mut addresses = Vec::new();
        addresses
            .try_reserve_exact(sizes.len())
            .map_err(|_| MemoryError32::Allocation)?;
        let mut owned_sizes = Vec::new();
        owned_sizes
            .try_reserve_exact(sizes.len())
            .map_err(|_| MemoryError32::Allocation)?;
        owned_sizes.extend_from_slice(sizes);
        let mut extents = Vec::new();
        extents
            .try_reserve_exact(sizes.len())
            .map_err(|_| MemoryError32::Allocation)?;
        for (ordinal, &size) in sizes.iter().enumerate() {
            if source.maximum != 0 && size >= 0x7fff8 {
                return Err(MemoryError32::Capacity);
            }
            let extent = self.extent(heap, size)?;
            let index = candidate
                .iter()
                .enumerate()
                .filter(|(_, block)| block.size.is_none() && block.extent >= extent)
                .min_by_key(|(_, block)| block.extent)
                .map(|(index, _)| index)
                .ok_or(MemoryError32::Capacity)?;
            let old = candidate[index];
            candidate[index] = Block32 {
                address: old.address,
                extent,
                size: Some(size),
                allocation: self.next_allocation + ordinal as u64 + 1,
            };
            if old.extent > extent {
                candidate.insert(
                    index + 1,
                    Block32 {
                        address: old.address + extent,
                        extent: old.extent - extent,
                        size: None,
                        allocation: 0,
                    },
                );
            }
            addresses.push(old.address);
            extents.push((old.address, extent));
        }
        let other = self
            .heaps
            .iter()
            .enumerate()
            .filter(|&(index, _)| index != heap)
            .map(|(_, heap)| heap.blocks.len())
            .sum::<usize>();
        if other
            .checked_add(candidate.len())
            .is_none_or(|count| count > self.limits.blocks)
        {
            return Err(MemoryError32::Capacity);
        }
        Ok(PreparedHeapAllocations32 {
            handle,
            original,
            candidate,
            addresses,
            sizes: owned_sizes,
            extents,
            flags: source.flags,
            maximum: source.maximum,
            small: source.small.is_some(),
            identity: memory.identity(),
            version: memory.transaction_version(),
            previous_allocation: self.next_allocation,
            next_allocation,
        })
    }
    pub(crate) fn commit_initialized_allocations(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        prepared: PreparedHeapAllocations32,
        payloads: &[&[u8]],
        extra_writes: &[(GuestAddress, &[u8])],
    ) -> Result<(), MemoryError32> {
        let heap = self.index(prepared.handle)?;
        let source = &self.heaps[heap];
        if memory.identity() != prepared.identity
            || memory.transaction_version() != prepared.version
            || self.next_allocation != prepared.previous_allocation
            || source.blocks.len() != prepared.original.len()
            || source.blocks.iter().zip(&prepared.original).any(|(a, b)| {
                a.address != b.address
                    || a.extent != b.extent
                    || a.size != b.size
                    || a.allocation != b.allocation
            })
            || source.flags != prepared.flags
            || source.maximum != prepared.maximum
            || source.small.is_some() != prepared.small
            || payloads.len() != prepared.sizes.len()
            || payloads
                .iter()
                .zip(&prepared.sizes)
                .any(|(bytes, &size)| bytes.len() > size as usize)
        {
            return Err(MemoryError32::InvalidParameter);
        }
        let other = self
            .heaps
            .iter()
            .enumerate()
            .filter(|&(index, _)| index != heap)
            .map(|(_, heap)| heap.blocks.len())
            .sum::<usize>();
        if other
            .checked_add(prepared.candidate.len())
            .is_none_or(|count| count > self.limits.blocks)
        {
            return Err(MemoryError32::Capacity);
        }
        let mut writes = Vec::new();
        writes
            .try_reserve_exact(
                payloads
                    .len()
                    .checked_add(extra_writes.len())
                    .ok_or(MemoryError32::Capacity)?,
            )
            .map_err(|_| MemoryError32::Allocation)?;
        for (&address, &bytes) in prepared.addresses.iter().zip(payloads) {
            writes.push((GuestAddress(address), bytes));
        }
        writes.extend_from_slice(extra_writes);
        vm.commit_heap_ranges(
            memory,
            prepared.handle,
            &prepared.extents,
            if prepared.flags & 0x40000 != 0 {
                0x40
            } else {
                4
            },
            &writes,
        )?;
        self.heaps[heap].blocks = prepared.candidate;
        self.next_allocation = prepared.next_allocation;
        self.rebuild_small_blocks(heap);
        Ok(())
    }
}
pub(super) fn coalesce_blocks(blocks: &mut Vec<Block32>) {
    let mut count = 0usize;
    for position in 0..blocks.len() {
        let block = blocks[position];
        if count != 0 && blocks[count - 1].size.is_none() && block.size.is_none() {
            blocks[count - 1].extent += block.extent;
        } else {
            blocks[count] = block;
            count += 1;
        }
    }
    blocks.truncate(count);
}

#[cfg(test)]
mod tests;
