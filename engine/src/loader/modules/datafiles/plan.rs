use super::{DataFileLayout32, DataFileModule32, mapping};
use crate::{
    loader::{
        ModuleCatalog32, ModuleError32, ModuleId32, ModuleSet32, PreparedImagePage32,
        normalize_module_identity,
    },
    memory::{AddressSpace, PAGE_SIZE, PageInitialization, PageRange},
};
use std::sync::Arc;
pub struct DataFilePlan32 {
    record: DataFileModule32,
    pages: Vec<PreparedImagePage32>,
    revision: u64,
    memory_version: u64,
}
impl ModuleCatalog32 {
    pub fn plan_datafile(
        &self,
        set: &ModuleSet32,
        memory: &AddressSpace,
        name: &str,
        layout: DataFileLayout32,
        reserved: &[PageRange],
    ) -> Result<DataFilePlan32, ModuleError32> {
        if set.modules.len() + set.datafile_count >= self.limits.modules {
            return Err(ModuleError32::Capacity);
        }
        let source = self.source(&normalize_module_identity(name)?)?;
        let super::super::catalog::ModuleSource32::Guest { image, bytes, .. } = &*source else {
            return Err(ModuleError32::InvalidModuleKind);
        };
        let size = if layout == DataFileLayout32::File {
            u32::try_from(bytes.len()).map_err(|_| ModuleError32::Capacity)?
        } else {
            image.image_size
        };
        let required = if layout == DataFileLayout32::File {
            bytes.len().div_ceil(PAGE_SIZE as usize)
        } else {
            image.mapped_pages as usize
        };
        if required > (memory.capacity_pages() - memory.mapped_pages()) as usize {
            return Err(ModuleError32::Capacity);
        }
        let extent =
            u32::try_from(u64::from(size).div_ceil(u64::from(PAGE_SIZE)) * u64::from(PAGE_SIZE))
                .map_err(|_| ModuleError32::Capacity)?;
        let base = super::super::plan::place(
            memory,
            set,
            &[],
            reserved,
            image.preferred_base,
            extent,
            None,
        )?;
        let id =
            ModuleId32::from_sequence(set.next_id.checked_add(1).ok_or(ModuleError32::Capacity)?);
        set.revision.checked_add(1).ok_or(ModuleError32::Capacity)?;
        let (pages, ranges) = mapping::pages(&source, base, layout)?;
        if pages.len() > (memory.capacity_pages() - memory.mapped_pages()) as usize {
            return Err(ModuleError32::Capacity);
        }
        let mut witnesses = Vec::new();
        witnesses
            .try_reserve_exact(pages.len())
            .map_err(|_| ModuleError32::Allocation)?;
        witnesses.extend(pages.iter().map(|page| (page.address, 0)));
        Ok(DataFilePlan32 {
            record: DataFileModule32 {
                id,
                source,
                set_identity: Arc::clone(&set.identity),
                base,
                extent,
                layout,
                ranges,
                memory_identity: memory.identity(),
                witnesses,
            },
            pages,
            revision: set.revision,
            memory_version: memory.transaction_version(),
        })
    }
}
impl DataFilePlan32 {
    pub fn handle(&self) -> u32 {
        self.record.handle()
    }
    pub fn install(
        mut self,
        memory: &mut AddressSpace,
        set: &mut ModuleSet32,
    ) -> Result<DataFileModule32, ModuleError32> {
        if memory.identity() != self.record.memory_identity
            || memory.transaction_version() != self.memory_version
            || set.revision != self.revision
            || !Arc::ptr_eq(&set.identity, &self.record.set_identity)
        {
            return Err(ModuleError32::StalePlan);
        }
        let revision = set.revision.checked_add(1).ok_or(ModuleError32::Capacity)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(self.pages.len())
            .map_err(|_| ModuleError32::Allocation)?;
        rows.extend(self.pages.iter().map(|page| PageInitialization {
            address: page.address,
            permissions: page.permissions,
            bytes: page.bytes.as_slice(),
        }));
        memory.install_pages_with_word_writes(&rows, &[])?;
        for (address, witness) in &mut self.record.witnesses {
            *witness = memory
                .mapping_version_at(*address)
                .expect("installed resource mapping");
        }
        set.next_id = self.record.id.get();
        set.revision = revision;
        set.datafile_count += 1;
        Ok(self.record)
    }
}
