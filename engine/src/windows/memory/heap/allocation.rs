use super::*;

pub(crate) struct PreparedHeapAllocation32 {
    handle: u32,
    block: usize,
    original: Block32,
    extent: u32,
    size: u32,
    allocation: u64,
    identity: u64,
    version: u64,
}
impl PreparedHeapAllocation32 {
    pub(crate) fn address(&self) -> u32 {
        self.original.address
    }
}

impl HeapManager32 {
    pub(crate) fn allocate_initialized(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        handle: u32,
        bytes: &[u8],
    ) -> Result<u32, MemoryError32> {
        let size = u32::try_from(bytes.len()).map_err(|_| MemoryError32::Capacity)?;
        self.allocate_bytes(vm, memory, handle, 0, size, Some(bytes))
    }
    pub(crate) fn allocate(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        handle: u32,
        flags: u32,
        size: u32,
    ) -> Result<u32, MemoryError32> {
        self.allocate_bytes(vm, memory, handle, flags, size, None)
    }
    fn allocate_bytes(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        handle: u32,
        flags: u32,
        size: u32,
        initial: Option<&[u8]>,
    ) -> Result<u32, MemoryError32> {
        if flags & !(HEAP_NO_SERIALIZE | HEAP_GENERATE_EXCEPTIONS | HEAP_ZERO_MEMORY) != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let prepared = self.prepare_initialized_allocation(memory, handle, size)?;
        let mut bytes = if flags & HEAP_ZERO_MEMORY != 0 {
            buffer(size as usize)?
        } else {
            buffer(initial.map_or(0, |bytes| bytes.len()))?
        };
        if let Some(initial) = initial {
            bytes[..initial.len()].copy_from_slice(initial);
        }
        self.commit_initialized_allocation(vm, memory, prepared, &bytes, &[])
    }
    pub(crate) fn prepare_initialized_allocation(
        &mut self,
        memory: &AddressSpace,
        handle: u32,
        size: u32,
    ) -> Result<PreparedHeapAllocation32, MemoryError32> {
        let heap = self.index(handle)?;
        let allocation = self
            .next_allocation
            .checked_add(1)
            .ok_or(MemoryError32::Capacity)?;
        if self.heaps[heap].maximum != 0 && size >= 0x7fff8 {
            return Err(MemoryError32::Capacity);
        }
        let extent = self.extent(heap, size)?;
        let block = self.heaps[heap]
            .small
            .as_ref()
            .and_then(|small| small.find(extent, &self.heaps[heap].blocks))
            .or_else(|| {
                self.heaps[heap].free.find(extent).map(|address| {
                    self.heaps[heap]
                        .blocks
                        .binary_search_by_key(&address, |block| block.address)
                        .expect("indexed free block remains in the heap")
                })
            })
            .ok_or(MemoryError32::Capacity)?;
        let original = self.heaps[heap].blocks[block];
        if original.extent > extent {
            self.reserve_block(heap)?;
        }
        Ok(PreparedHeapAllocation32 {
            handle,
            block,
            original,
            extent,
            size,
            allocation,
            identity: memory.identity(),
            version: memory.transaction_version(),
        })
    }
    pub(crate) fn commit_initialized_allocation(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        prepared: PreparedHeapAllocation32,
        bytes: &[u8],
        extra_writes: &[(GuestAddress, &[u8])],
    ) -> Result<u32, MemoryError32> {
        let heap = self.index(prepared.handle)?;
        let Some(current) = self.heaps[heap].blocks.get(prepared.block) else {
            return Err(MemoryError32::InvalidParameter);
        };
        if memory.identity() != prepared.identity
            || memory.transaction_version() != prepared.version
            || self.next_allocation.checked_add(1) != Some(prepared.allocation)
            || current.address != prepared.original.address
            || current.extent != prepared.original.extent
            || current.size.is_some()
            || bytes.len() > prepared.size as usize
        {
            return Err(MemoryError32::InvalidParameter);
        }
        let protection = if self.heaps[heap].flags & 0x40000 != 0 {
            0x40
        } else {
            4
        };
        let initial = (GuestAddress(prepared.original.address), bytes);
        if extra_writes.is_empty() {
            vm.allocate_with_writes(
                memory,
                &[],
                prepared.original.address,
                prepared.extent,
                MEM_COMMIT,
                protection,
                &[initial],
            )?;
        } else {
            let mut writes = Vec::new();
            writes
                .try_reserve_exact(extra_writes.len() + 1)
                .map_err(|_| MemoryError32::Allocation)?;
            writes.push(initial);
            writes.extend_from_slice(extra_writes);
            vm.allocate_with_writes(
                memory,
                &[],
                prepared.original.address,
                prepared.extent,
                MEM_COMMIT,
                protection,
                &writes,
            )?;
        }
        let original = prepared.original;
        self.heaps[heap].free.remove(original);
        self.next_allocation = prepared.allocation;
        self.heaps[heap].blocks[prepared.block] = Block32 {
            address: original.address,
            extent: prepared.extent,
            size: Some(prepared.size),
            allocation: prepared.allocation,
        };
        if original.extent > prepared.extent {
            let remainder = Block32 {
                address: original.address + prepared.extent,
                extent: original.extent - prepared.extent,
                size: None,
                allocation: 0,
            };
            self.heaps[heap]
                .blocks
                .insert(prepared.block + 1, remainder);
            self.heaps[heap].free.insert(remainder);
        }
        self.record_small_allocation(heap, original, prepared.extent);
        Ok(original.address)
    }
    pub(crate) fn reallocate(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        handle: u32,
        flags: u32,
        pointer: u32,
        size: u32,
    ) -> Result<u32, MemoryError32> {
        if flags
            & !(HEAP_NO_SERIALIZE
                | HEAP_GENERATE_EXCEPTIONS
                | HEAP_ZERO_MEMORY
                | HEAP_REALLOC_IN_PLACE_ONLY)
            != 0
        {
            return Err(MemoryError32::InvalidParameter);
        }
        let heap = self.index(handle)?;
        let block = self.block(heap, pointer)?;
        if self.heaps[heap].maximum != 0 && size >= 0x7fff8 {
            return Err(MemoryError32::Capacity);
        }
        let old = self.heaps[heap].blocks[block];
        let old_size = old.size.unwrap();
        let extent = self.extent(heap, size)?;
        let adjacent = self.heaps[heap]
            .blocks
            .get(block + 1)
            .copied()
            .filter(|block| block.size.is_none());
        let available = old.extent + adjacent.map_or(0, |block| block.extent);
        if extent <= available {
            let allocation = self
                .next_allocation
                .checked_add(1)
                .ok_or(MemoryError32::Capacity)?;
            if extent < old.extent {
                self.reserve_block(heap)?;
            }
            let zeros = if flags & HEAP_ZERO_MEMORY != 0 && size > old_size {
                buffer((size - old_size) as usize)?
            } else {
                Vec::new()
            };
            vm.allocate_with_writes(
                memory,
                &[],
                pointer,
                extent,
                MEM_COMMIT,
                if self.heaps[heap].flags & 0x40000 != 0 {
                    0x40
                } else {
                    4
                },
                &[(
                    GuestAddress(
                        pointer
                            .checked_add(old_size)
                            .ok_or(MemoryError32::InvalidParameter)?,
                    ),
                    &zeros,
                )],
            )?;
            if let Some(adjacent) = adjacent {
                self.heaps[heap].free.remove(adjacent);
                self.heaps[heap].blocks.remove(block + 1);
            }
            self.next_allocation = allocation;
            self.heaps[heap].blocks[block] = Block32 {
                address: pointer,
                extent,
                size: Some(size),
                allocation,
            };
            if extent < available {
                self.heaps[heap].blocks.insert(
                    block + 1,
                    Block32 {
                        address: pointer + extent,
                        extent: available - extent,
                        size: None,
                        allocation: 0,
                    },
                );
            }
            if extent < available {
                self.coalesce_at(heap, block + 1);
            } else {
                self.rebuild_small_blocks(heap);
            }
            return Ok(pointer);
        }
        if flags & HEAP_REALLOC_IN_PLACE_ONLY != 0 {
            return Err(MemoryError32::Capacity);
        }
        let mut bytes = buffer(old_size.min(size) as usize)?;
        memory.read(GuestAddress(pointer), &mut bytes)?;
        let new = self.allocate_bytes(
            vm,
            memory,
            handle,
            flags & !HEAP_REALLOC_IN_PLACE_ONLY,
            size,
            Some(&bytes),
        )?;
        self.free(handle, 0, pointer)
            .expect("reallocation source remains owned until destination publication");
        Ok(new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialized_allocation_and_extra_pointer_publish_in_one_neutral_transaction() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let before_pages = memory.mapped_pages();
        let before_version = memory.transaction_version();
        let before_regions = vm.query(&memory, 0x20000).unwrap();
        let prepared = heaps
            .prepare_initialized_allocation(&memory, heap, 8)
            .unwrap();
        let address = prepared.address();
        assert_eq!(
            heaps.commit_initialized_allocation(
                &mut vm,
                &mut memory,
                prepared,
                b"payload!",
                &[(GuestAddress(0xfffffff0), &address.to_le_bytes())]
            ),
            Err(MemoryError32::NoAccess)
        );
        assert_eq!(memory.mapped_pages(), before_pages);
        assert_eq!(memory.transaction_version(), before_version);
        assert_eq!(vm.query(&memory, 0x20000).unwrap(), before_regions);
        assert_eq!(
            heaps.size(heap, 0, address),
            Err(MemoryError32::InvalidParameter)
        );
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                crate::memory::Permissions::READ_WRITE,
            )
            .unwrap();
        let prepared = heaps
            .prepare_initialized_allocation(&memory, heap, 8)
            .unwrap();
        assert_eq!(prepared.address(), address);
        let pointer = heaps
            .commit_initialized_allocation(
                &mut vm,
                &mut memory,
                prepared,
                b"payload!",
                &[(GuestAddress(0x1000), &address.to_le_bytes())],
            )
            .unwrap();
        let mut bytes = [0; 8];
        memory.read(GuestAddress(pointer), &mut bytes).unwrap();
        assert_eq!(&bytes, b"payload!");
        let mut output = [0; 4];
        memory.read(GuestAddress(0x1000), &mut output).unwrap();
        assert_eq!(u32::from_le_bytes(output), pointer);
        heaps.local_free(pointer).unwrap();
    }
    #[test]
    fn initialized_allocation_rejects_an_exact_stale_memory_owner_before_publication() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let prepared = heaps
            .prepare_initialized_allocation(&memory, heap, 4)
            .unwrap();
        let pointer = prepared.address();
        let other = heaps.allocate(&mut vm, &mut memory, heap, 0, 4).unwrap();
        assert_eq!(pointer, other);
        heaps.free(heap, 0, other).unwrap();
        let version = memory.transaction_version();
        assert_eq!(
            heaps.commit_initialized_allocation(&mut vm, &mut memory, prepared, b"old!", &[]),
            Err(MemoryError32::InvalidParameter)
        );
        assert_eq!(memory.transaction_version(), version);
    }
}
