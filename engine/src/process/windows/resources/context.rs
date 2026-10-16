use super::{ResourceConfig32, ResourceError32 as E};
use crate::{
    loader::{Directory32, ModuleId32},
    process::EngineInstance,
};
#[derive(Clone, Copy)]
pub(super) struct ModuleResource32 {
    pub id: ModuleId32,
    pub base: u32,
    pub handle: u32,
    pub image_size: u32,
    pub directory: Directory32,
}
#[derive(Clone, Copy)]
pub(super) struct ResourceHandle32 {
    pub value: u32,
    pub module: ModuleId32,
    pub entry_rva: u32,
    pub loaded: bool,
}
pub(in crate::process) struct ResourceRuntime32 {
    pub(super) config: ResourceConfig32,
    pub(super) handles: Vec<Option<ResourceHandle32>>,
    pub(super) next_handle: u32,
    pub(super) pending: Vec<Option<super::enumeration::PendingEnum32>>,
    pub(super) next_continuation: u64,
    pub(super) enum_bytes: usize,
}
impl ResourceRuntime32 {
    pub(in crate::process) fn new(config: ResourceConfig32) -> Result<Self, E> {
        if config.scratch.first == 0
            || !(1..=65536).contains(&config.max_handles)
            || !(1..=64).contains(&config.max_callbacks)
            || !(4096..=64 * 1024 * 1024).contains(&config.max_enum_bytes)
        {
            return Err(E::InvalidConfiguration);
        }
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(config.max_handles as usize)
            .map_err(|_| E::Allocation)?;
        handles.resize_with(config.max_handles as usize, || None);
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(config.max_callbacks as usize)
            .map_err(|_| E::Allocation)?;
        pending.resize_with(config.max_callbacks as usize, || None);
        Ok(Self {
            config,
            handles,
            next_handle: 0x57000000,
            pending,
            next_continuation: 0,
            enum_bytes: 0,
        })
    }
    pub(in crate::process) fn scratch_reservation(&self) -> crate::memory::PageRange {
        self.config.scratch
    }
    pub(super) fn cache(&mut self, module: ModuleId32, entry_rva: u32) -> Result<u32, E> {
        if let Some(handle) = self
            .handles
            .iter()
            .flatten()
            .find(|handle| handle.module == module && handle.entry_rva == entry_rva)
        {
            return Ok(handle.value);
        }
        let slot = self
            .handles
            .iter()
            .position(Option::is_none)
            .ok_or(E::Capacity)?;
        let value = self
            .next_handle
            .checked_add(4)
            .filter(|&value| value < 0x80000000)
            .ok_or(E::Capacity)?;
        self.handles[slot] = Some(ResourceHandle32 {
            value,
            module,
            entry_rva,
            loaded: false,
        });
        self.next_handle = value;
        Ok(value)
    }
    pub(super) fn handle(&self, value: u32) -> Result<ResourceHandle32, E> {
        self.handles
            .iter()
            .flatten()
            .find(|handle| handle.value == value)
            .copied()
            .ok_or(E::InvalidHandle)
    }
    pub(super) fn prune(&mut self, live: &[ModuleId32]) {
        for handle in &mut self.handles {
            if handle.is_some_and(|handle| !live.contains(&handle.module)) {
                *handle = None;
            }
        }
    }
}
impl EngineInstance {
    pub(super) fn resource_module(&self, raw: u32) -> Result<ModuleResource32, E> {
        let base = if raw == 0 {
            self.module_handle(None)
                .map_err(|_| E::InvalidHandle)?
                .ok_or(E::InvalidHandle)?
        } else {
            raw
        };
        if let Some(module) = self
            .resource_only_modules()
            .find(|module| module.handle() == base)
        {
            module
                .check(self.memory.as_ref().ok_or(E::NotConfigured)?)
                .map_err(|_| E::InvalidHandle)?;
            return Ok(ModuleResource32 {
                id: module.id(),
                base: module.base(),
                handle: module.handle(),
                image_size: module.image_size(),
                directory: module
                    .resource_directory()
                    .ok_or(E::Pe(crate::loader::ResourceError32::TypeNotFound))?,
            });
        }
        let module = self
            .loaded_windows_modules()
            .find(|module| module.base() == base)
            .ok_or(E::InvalidHandle)?;
        Ok(ModuleResource32 {
            id: module.id(),
            base,
            handle: base,
            image_size: module.image_size(),
            directory: module
                .resource_directory()
                .ok_or(E::Pe(crate::loader::ResourceError32::TypeNotFound))?,
        })
    }
    pub(super) fn resource_module_id(&self, id: ModuleId32) -> Result<ModuleResource32, E> {
        if let Some(module) = self.resource_only_module(id) {
            return self.resource_module(module.handle());
        }
        let base = self
            .loaded_windows_modules()
            .find(|module| module.id() == id)
            .ok_or(E::InvalidHandle)?
            .base();
        self.resource_module(base)
    }
    pub(super) fn resource_reader(
        &self,
        module: ModuleResource32,
    ) -> Result<crate::loader::PeResources32<'_>, E> {
        let memory = self.memory.as_ref().ok_or(E::NotConfigured)?;
        if let Some(datafile) = self.resource_only_module(module.id) {
            return Ok(crate::loader::PeResources32::from_datafile(
                memory,
                datafile,
                crate::loader::ParserLimits32::default(),
            )?);
        }
        Ok(crate::loader::PeResources32::new(
            memory,
            module.base,
            module.image_size,
            module.directory,
            crate::loader::ParserLimits32::default(),
        )?)
    }
}
