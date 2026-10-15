use super::{super::DispatchError32, GuestLock32, LockRegistry32};
use crate::{
    graphics::d3d8::api::HResult,
    memory::{
        AddressSpace, GuestAddress, PAGE_SIZE, PageInitialization, PageRange, Permissions,
        WordWrite32,
    },
};

impl LockRegistry32 {
    pub(super) fn preflight(&self, memory: &AddressSpace) -> Result<(), DispatchError32> {
        if self.entries.len() == self.limit {
            return Err(HResult::OUT_OF_MEMORY.into());
        }
        if self
            .memory_identity
            .is_some_and(|identity| identity != memory.identity())
        {
            return Err(HResult::INVALID_CALL.into());
        }
        Ok(())
    }

    pub(super) fn publish(
        &mut self,
        memory: &mut AddressSpace,
        resource: crate::graphics::ResourceId,
        info: crate::graphics::d3d8::resources::LockInfo,
        bytes: &[u8],
        output: u32,
        volume: Option<bool>,
    ) -> Result<(), DispatchError32> {
        self.preflight(memory)?;
        let data_offset = if volume.is_none() { info.offset } else { 0 };
        if volume.is_none()
            && let Some(existing) = self.entries.iter().find(|entry| entry.resource == resource)
        {
            self.validate_mapping(memory, existing)?;
            let range = existing.range;
            let mapping = existing.mapping;
            let address = (range.first * PAGE_SIZE as usize) as u32 + data_offset;
            memory.write_words32(&[WordWrite32 {
                address: GuestAddress(output),
                value: address,
            }])?;
            self.entries.push(GuestLock32 {
                resource,
                info,
                range,
                mapping,
                data_offset,
            });
            return Ok(());
        }
        let count = bytes.len().div_ceil(PAGE_SIZE as usize);
        let end = self
            .next_page
            .checked_add(count)
            .filter(|end| *end <= self.range.first + self.range.count)
            .ok_or(HResult::OUT_OF_MEMORY)?;
        let address = GuestAddress((self.next_page * PAGE_SIZE as usize) as u32);
        let range = PageRange::new(address, count as u32)?;
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(count)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for (index, page) in bytes.chunks(PAGE_SIZE as usize).enumerate() {
            pages.push(PageInitialization {
                address: GuestAddress(address.0 + index as u32 * PAGE_SIZE),
                permissions: Permissions::READ_WRITE,
                bytes: page,
            });
        }
        let mut words = [WordWrite32 {
            address: GuestAddress(output),
            value: 0,
        }; 3];
        let count = match volume {
            None => {
                words[0].value = address.0 + data_offset;
                1
            }
            Some(false) => {
                words[0].value = info.row_pitch;
                words[1] = WordWrite32 {
                    address: GuestAddress(output + 4),
                    value: address.0,
                };
                2
            }
            Some(true) => {
                words[0].value = info.row_pitch;
                words[1] = WordWrite32 {
                    address: GuestAddress(output + 4),
                    value: info.slice_pitch,
                };
                words[2] = WordWrite32 {
                    address: GuestAddress(output + 8),
                    value: address.0,
                };
                3
            }
        };
        memory
            .install_pages_with_word_writes(&pages, &words[..count])
            .map_err(|error| match error {
                crate::memory::MemoryError::Capacity | crate::memory::MemoryError::Allocation => {
                    DispatchError32::Api(HResult::OUT_OF_MEMORY)
                }
                error => error.into(),
            })?;
        self.entries.push(GuestLock32 {
            resource,
            info,
            range,
            mapping: memory.transaction_version(),
            data_offset,
        });
        self.next_page = end;
        self.memory_identity = Some(memory.identity());
        Ok(())
    }

    pub(super) fn validate_mapping(
        &self,
        memory: &AddressSpace,
        entry: &GuestLock32,
    ) -> Result<(), DispatchError32> {
        if self.memory_identity != Some(memory.identity()) {
            return Err(HResult::INVALID_CALL.into());
        }
        for page in entry.range.first..entry.range.first + entry.range.count {
            let address = GuestAddress((page * PAGE_SIZE as usize) as u32);
            if memory.mapping_version_at(address)? != entry.mapping
                || memory.permissions_at(address)? != Permissions::READ_WRITE
            {
                return Err(HResult::INVALID_CALL.into());
            }
        }
        Ok(())
    }
}
