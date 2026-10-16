use crate::{
    loader::modules::catalog::ModuleSource32,
    loader::{Directory32, ModuleError32, ModuleId32, ModuleSet32},
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions},
};
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFileLayout32 {
    File,
    Image,
}
#[derive(Debug)]
pub struct DataFileModule32 {
    pub(super) id: ModuleId32,
    pub(super) source: Arc<ModuleSource32>,
    pub(super) set_identity: Arc<()>,
    pub(super) base: u32,
    pub(super) extent: u32,
    pub(super) layout: DataFileLayout32,
    pub(super) ranges: Vec<PageRange>,
    pub(super) memory_identity: u64,
    pub(super) witnesses: Vec<(GuestAddress, u64)>,
}
impl DataFileModule32 {
    pub fn id(&self) -> ModuleId32 {
        self.id
    }
    pub fn base(&self) -> u32 {
        self.base
    }
    pub fn extent(&self) -> u32 {
        self.extent
    }
    pub fn layout(&self) -> DataFileLayout32 {
        self.layout
    }
    pub fn handle(&self) -> u32 {
        self.base
            | if self.layout == DataFileLayout32::File {
                1
            } else {
                2
            }
    }
    pub fn identity(&self) -> &str {
        self.source.name()
    }
    pub fn mapped_ranges(&self) -> &[PageRange] {
        &self.ranges
    }
    pub fn image_size(&self) -> u32 {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.image_size,
            _ => unreachable!(),
        }
    }
    pub fn resource_directory(&self) -> Option<Directory32> {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.resource_directory(),
            _ => None,
        }
    }
    pub fn reservation(&self) -> PageRange {
        PageRange::new(GuestAddress(self.base), self.extent / PAGE_SIZE)
            .expect("validated datafile extent")
    }
    pub fn check(&self, memory: &AddressSpace) -> Result<(), ModuleError32> {
        if memory.identity() != self.memory_identity {
            return Err(ModuleError32::StalePlan);
        }
        for &(address, version) in &self.witnesses {
            if memory.mapping_version_at(address)? != version {
                return Err(ModuleError32::StalePlan);
            }
        }
        Ok(())
    }
    pub fn address(&self, rva: u32, length: u32) -> Result<GuestAddress, ModuleError32> {
        let end = u64::from(rva) + u64::from(length);
        let ModuleSource32::Guest { image, bytes, .. } = &*self.source else {
            unreachable!()
        };
        if end > u64::from(image.image_size) {
            return Err(ModuleError32::InvalidRequest);
        }
        let offset = if self.layout == DataFileLayout32::Image
            || end <= u64::from(image.header_size)
        {
            rva
        } else {
            let section = image
                .sections
                .iter()
                .find(|section| {
                    rva >= section.rva && end <= u64::from(section.rva) + section.raw.len() as u64
                })
                .ok_or(ModuleError32::InvalidRequest)?;
            u32::try_from(section.raw.start)
                .map_err(|_| ModuleError32::Capacity)?
                .checked_add(rva - section.rva)
                .ok_or(ModuleError32::InvalidRequest)?
        };
        if self.layout == DataFileLayout32::File
            && u64::from(offset) + u64::from(length) > bytes.len() as u64
        {
            return Err(ModuleError32::InvalidRequest);
        }
        Ok(GuestAddress(
            self.base
                .checked_add(offset)
                .ok_or(ModuleError32::InvalidRequest)?,
        ))
    }
    pub fn permits(
        &self,
        memory: &AddressSpace,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<(), ModuleError32> {
        self.check(memory)?;
        if permissions.allows(Access::Write) || permissions.allows(Access::Execute) {
            return Err(ModuleError32::InvalidRequest);
        }
        for page in range.indices() {
            if self
                .witnesses
                .binary_search_by_key(&(page as u32 * PAGE_SIZE), |(address, _)| address.0)
                .is_err()
            {
                return Err(ModuleError32::InvalidRequest);
            }
        }
        Ok(())
    }
    pub(crate) fn refresh_protection(&mut self, memory: &AddressSpace, range: PageRange) {
        for (address, version) in &mut self.witnesses {
            if range
                .indices()
                .contains(&(address.0 as usize / PAGE_SIZE as usize))
            {
                *version = memory
                    .mapping_version_at(*address)
                    .expect("preflighted resource protection");
            }
        }
    }
    pub fn unload(
        &self,
        memory: &mut AddressSpace,
        set: &mut ModuleSet32,
    ) -> Result<(), ModuleError32> {
        self.check(memory)?;
        if !Arc::ptr_eq(&self.set_identity, &set.identity) || set.datafile_count == 0 {
            return Err(ModuleError32::StalePlan);
        }
        let revision = set.revision.checked_add(1).ok_or(ModuleError32::Capacity)?;
        memory.unmap_ranges_with_word_writes(&self.ranges, &[])?;
        set.revision = revision;
        set.datafile_count -= 1;
        Ok(())
    }
}
