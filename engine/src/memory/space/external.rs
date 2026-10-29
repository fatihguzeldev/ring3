use super::*;
use crate::abi::external_memory::ExternalMemory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExternalCodePatch {
    pub(crate) address: GuestAddress,
    pub(crate) mapping: u64,
    pub(crate) slot: u32,
    pub(crate) content: u64,
}

pub(super) struct MappingChanges {
    pub(super) pages: Vec<u32>,
    bits: Box<[u64]>,
}

impl MappingChanges {
    pub(super) fn new() -> Result<Self, MemoryError> {
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(GUEST_PAGES)
            .map_err(|_| MemoryError::Allocation)?;
        Ok(Self {
            pages,
            bits: filled(GUEST_PAGES / 64, 0)?.into_boxed_slice(),
        })
    }

    fn clear(&mut self) {
        for &page in &self.pages {
            self.bits[page as usize / 64] &= !(1u64 << (page % 64));
        }
        self.pages.clear();
    }

    fn mark(&mut self, page: usize) {
        let bit = 1u64 << (page % 64);
        if self.bits[page / 64] & bit == 0 {
            self.bits[page / 64] |= bit;
            self.pages.push(page as u32);
        }
    }
}

impl AddressSpace {
    pub(crate) fn external_code_patch(
        &self,
        address: GuestAddress,
        expected: [u8; 2],
    ) -> Result<ExternalCodePatch, MemoryError> {
        let page = address.0 as usize / PAGE_SIZE as usize;
        let within = address.0 as usize % PAGE_SIZE as usize;
        let mapping = self.mappings[page];
        if within > PAGE_SIZE as usize - 2
            || mapping == 0
            || !self.permissions[page].allows(Access::Execute)
            || self
                .private_slots
                .as_ref()
                .is_some_and(|slots| slots[page] != 0)
        {
            return Err(MemoryError::InvalidRange);
        }
        let slot = mapping as usize - 1;
        let offset = slot * PAGE_SIZE as usize + within;
        if self.references[slot] != 1 || self.backing[offset..offset + 2] != expected {
            return Err(MemoryError::InvalidRange);
        }
        Ok(ExternalCodePatch {
            address,
            mapping: self.mapping_versions[page],
            slot: slot as u32,
            content: self.versions[slot].content,
        })
    }

    pub(crate) fn external_code_patch_matches(
        &self,
        patch: &ExternalCodePatch,
        expected: [u8; 2],
    ) -> bool {
        self.external_code_patch(patch.address, expected).as_ref() == Ok(patch)
    }

    pub(crate) fn external_patch_code(
        &mut self,
        patches: &[ExternalCodePatch],
        expected: [u8; 2],
        replacement: [u8; 2],
    ) -> Result<u64, MemoryError> {
        let mut end = 0u64;
        for patch in patches {
            if u64::from(patch.address.0) < end
                || !self.external_code_patch_matches(patch, expected)
            {
                return Err(MemoryError::InvalidRange);
            }
            end = u64::from(patch.address.0) + 2;
        }
        if patches.is_empty() {
            return Ok(self.version);
        }
        let version = self.advance_version()?;
        for patch in patches {
            let offset = patch.slot as usize * PAGE_SIZE as usize
                + patch.address.0 as usize % PAGE_SIZE as usize;
            self.backing[offset..offset + 2].copy_from_slice(&replacement);
            self.versions[patch.slot as usize].content = version;
        }
        self.code_validation_version = version;
        self.code_source_version = version;
        self.external_executable_epoch = version;
        Ok(version)
    }

    pub(crate) fn external_write_is_stable(&self, address: GuestAddress, length: usize) -> bool {
        if length == 0 {
            return true;
        }
        let Some(end) = u64::from(address.0)
            .checked_add(length as u64)
            .filter(|end| *end <= ADDRESS_LIMIT)
        else {
            return false;
        };
        let first = address.0 as usize / PAGE_SIZE as usize;
        let last = ((end - 1) / u64::from(PAGE_SIZE)) as usize;
        (first..=last).all(|page| {
            let mapping = self.mappings[page];
            mapping != 0
                && self.permissions[page].allows(Access::Write)
                && self
                    .private_slots
                    .as_ref()
                    .is_none_or(|slots| slots[page] == 0)
                && self.executable_views[mapping as usize - 1] == 0
        })
    }

    pub(super) fn record_external_mapping_change(&mut self, page: usize, version: u64) {
        self.external_mapping_epoch = version;
        if let Some(changes) = self.external_mapping_changes.as_mut() {
            changes.mark(page);
        }
    }

    pub(crate) fn acknowledge_external_mappings(
        &mut self,
        epoch: u64,
        count: u32,
    ) -> Result<(), MemoryError> {
        let changes = self
            .external_mapping_changes
            .as_mut()
            .ok_or(MemoryError::InvalidRange)?;
        if epoch != self.external_mapping_epoch || count as usize != changes.pages.len() {
            return Err(MemoryError::InvalidRange);
        }
        if let Some(collector) = self.external_dirty.as_mut() {
            for &page in &changes.pages {
                let page = page as usize;
                collector.update_mapping(
                    page,
                    self.mappings[page],
                    self.permissions[page],
                    self.private_slots
                        .as_ref()
                        .is_some_and(|slots| slots[page] != 0),
                );
            }
        }
        changes.clear();
        Ok(())
    }

    pub(crate) fn attach_external_backing(
        &mut self,
        backing: ExternalMemory,
    ) -> Result<(), MemoryError> {
        let changes = MappingChanges::new()?;
        self.backing.attach_external(backing)?;
        self.external_mapping_changes = Some(changes);
        self.external_dirty = None;
        Ok(())
    }

    pub(crate) fn release_external_backing(&mut self) -> Result<(), MemoryError> {
        self.backing.release_external()?;
        self.external_mapping_changes = None;
        self.external_dirty = None;
        Ok(())
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
            (112, self.external_executable_epoch),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        // collector, epochs, gate flags and optional global RX PTEs are available before attach.
        bytes[108..112].copy_from_slice(&62u32.to_le_bytes());
        if let Some(changes) = self.external_mapping_changes.as_ref() {
            for (offset, value) in [
                (96, changes.pages.as_ptr() as u32),
                (100, changes.pages.len() as u32),
                (104, GUEST_PAGES as u32),
                (108, 63),
            ] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
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
                self.external_executable_epoch = version;
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
