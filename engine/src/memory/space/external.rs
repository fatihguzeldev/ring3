use super::*;
use crate::abi::external_memory::ExternalMemory;

impl AddressSpace {
    pub(crate) fn attach_external_backing(
        &mut self,
        backing: ExternalMemory,
    ) -> Result<(), MemoryError> {
        self.backing.attach_external(backing)
    }

    pub(crate) fn release_external_backing(&mut self) -> Result<(), MemoryError> {
        self.backing.release_external()
    }

    pub(crate) fn external_descriptor(&mut self) -> [u8; 128] {
        let view = self.core_view();
        let mut bytes = [0; 128];
        crate::abi::header::write(&mut bytes, *b"R3CV");
        for (index, value) in [
            view.backing,
            self.backing.len(),
            view.mappings,
            view.permissions,
            GUEST_PAGES,
            self.capacity_pages() as usize,
            self.mapped_pages() as usize,
            self.mapping_versions.as_ptr() as usize,
            view.page_versions,
            view.private_slots,
            view.executable_views,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[16 + index * 4..20 + index * 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        for (offset, value) in [
            (64, self.identity),
            (72, self.version),
            (80, self.code_validation_version),
            (88, self.external_mapping_epoch),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub(crate) fn publish_external_dirty(&mut self, slots: &[u32]) -> Result<(), MemoryError> {
        if slots.is_empty() {
            return Ok(());
        }
        if slots.iter().any(|&slot| {
            self.references
                .get(slot as usize)
                .is_none_or(|&count| count == 0)
        }) {
            return Err(MemoryError::InvalidRange);
        }
        let version = self.advance_version()?;
        for &slot in slots {
            self.versions[slot as usize].content = version;
            if self.executable_views[slot as usize] != 0 {
                self.code_validation_version = version;
                self.code_source_version = version;
            }
        }
        Ok(())
    }

    pub(crate) fn external_write_fault(&mut self, address: u32) -> Result<(), MemoryError> {
        let page = address as usize / PAGE_SIZE as usize;
        let slot = self.access_slot(page, GuestAddress(address), Access::Write)?;
        if self
            .private_slots
            .as_ref()
            .is_none_or(|slots| slots[page] == 0)
        {
            return Err(MemoryError::InvalidRange);
        }
        let version = self.advance_version()?;
        self.writable_slot_from(page, slot, version);
        Ok(())
    }
}
