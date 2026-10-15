use super::*;

impl HeapManager32 {
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
        if flags & HEAP_GENERATE_EXCEPTIONS != 0 {
            return Err(MemoryError32::Unsupported);
        }
        let heap = self.index(handle)?;
        if self.heaps[heap].maximum != 0 && size >= 0x7fff8 {
            return Err(MemoryError32::Capacity);
        }
        let extent = checked_size(size)?;
        let block = self.heaps[heap]
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block.size.is_none() && block.extent >= extent)
            .min_by_key(|(_, block)| block.extent)
            .map(|(index, _)| index)
            .ok_or(MemoryError32::Capacity)?;
        let original = self.heaps[heap].blocks[block];
        if original.extent > extent {
            self.reserve_block(heap)?;
        }
        let mut bytes = if flags & HEAP_ZERO_MEMORY != 0 {
            buffer(size as usize)?
        } else {
            buffer(initial.map_or(0, |bytes| bytes.len()))?
        };
        if let Some(initial) = initial {
            bytes[..initial.len()].copy_from_slice(initial);
        }
        vm.allocate_with_writes(
            memory,
            &[],
            original.address,
            extent,
            MEM_COMMIT,
            if self.heaps[heap].flags & 0x40000 != 0 {
                0x40
            } else {
                4
            },
            &[(GuestAddress(original.address), &bytes)],
        )?;
        self.heaps[heap].blocks[block] = Block32 {
            address: original.address,
            extent,
            size: Some(size),
        };
        if original.extent > extent {
            self.heaps[heap].blocks.insert(
                block + 1,
                Block32 {
                    address: original.address + extent,
                    extent: original.extent - extent,
                    size: None,
                },
            );
        }
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
        if flags & HEAP_GENERATE_EXCEPTIONS != 0 {
            return Err(MemoryError32::Unsupported);
        }
        let heap = self.index(handle)?;
        let block = self.block(heap, pointer)?;
        if self.heaps[heap].maximum != 0 && size >= 0x7fff8 {
            return Err(MemoryError32::Capacity);
        }
        let old = self.heaps[heap].blocks[block];
        let old_size = old.size.unwrap();
        let extent = checked_size(size)?;
        let adjacent = self.heaps[heap]
            .blocks
            .get(block + 1)
            .copied()
            .filter(|block| block.size.is_none());
        let available = old.extent + adjacent.map_or(0, |block| block.extent);
        if extent <= available {
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
            if adjacent.is_some() {
                self.heaps[heap].blocks.remove(block + 1);
            }
            self.heaps[heap].blocks[block] = Block32 {
                address: pointer,
                extent,
                size: Some(size),
            };
            if extent < available {
                self.heaps[heap].blocks.insert(
                    block + 1,
                    Block32 {
                        address: pointer + extent,
                        extent: available - extent,
                        size: None,
                    },
                );
            }
            self.coalesce(heap);
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
