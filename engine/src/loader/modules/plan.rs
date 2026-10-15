mod graph;
mod pages;
mod unload;

use std::sync::Arc;

use super::{LoadedModule32, ModuleError32, ModuleId32, ModuleSet32};
use crate::{
    loader::pe32::module::PreparedImagePage32,
    memory::{AddressSpace, GuestAddress, PageInitialization, Permissions, WordWrite32},
};

#[derive(Debug)]
pub struct ModuleLoad32 {
    pub root: ModuleId32,
    pub new_ids: Vec<ModuleId32>,
    pub initialization_order: Vec<ModuleId32>,
    pub introduced_edges: Vec<(ModuleId32, ModuleId32)>,
}

#[derive(Debug)]
pub struct LoadPlan32 {
    root: ModuleId32,
    memory_identity: u64,
    memory_version: u64,
    set_revision: u64,
    set_identity: Arc<()>,
    next_id: u64,
    additions: Vec<LoadedModule32>,
    pages: Vec<PreparedImagePage32>,
    initialization_order: Vec<ModuleId32>,
    dependency_updates: Vec<(ModuleId32, Vec<ModuleId32>)>,
    forwarder_limit: usize,
    reservations: Vec<(u32, u32)>,
}

impl LoadPlan32 {
    pub fn root(&self) -> ModuleId32 {
        self.root
    }

    pub fn additions(&self) -> &[LoadedModule32] {
        &self.additions
    }

    pub fn initialization_order(&self) -> &[ModuleId32] {
        &self.initialization_order
    }

    pub fn stage_write(
        &mut self,
        address: GuestAddress,
        bytes: &[u8],
    ) -> Result<(), ModuleError32> {
        pages::stage_write(&mut self.pages, address, bytes)
    }

    pub fn read_staged(
        &self,
        address: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), ModuleError32> {
        pages::read_staged(&self.pages, address, output)
    }

    pub fn add_page(
        &mut self,
        address: GuestAddress,
        permissions: Permissions,
        bytes: &[u8],
    ) -> Result<(), ModuleError32> {
        if !address.0.is_multiple_of(4096) || bytes.len() > 4096 {
            return Err(ModuleError32::InvalidRequest);
        }
        if self.pages.iter().any(|page| page.address == address) {
            return Err(ModuleError32::AddressConflict);
        }
        if self.intersects_image(address, 4096) {
            return Err(ModuleError32::AddressConflict);
        }
        let mut page = Box::new([0; 4096]);
        page[..bytes.len()].copy_from_slice(bytes);
        self.pages
            .try_reserve(1)
            .map_err(|_| ModuleError32::Allocation)?;
        let index = self
            .pages
            .partition_point(|page| page.address.0 < address.0);
        self.pages.insert(
            index,
            PreparedImagePage32 {
                address,
                permissions,
                bytes: page,
            },
        );
        Ok(())
    }

    pub fn intersects_image(&self, address: GuestAddress, length: u32) -> bool {
        let start = u64::from(address.0);
        let end = start + u64::from(length);
        self.reservations.iter().any(|(base, size)| {
            start < u64::from(*base) + u64::from(*size) && u64::from(*base) < end
        })
    }

    pub fn install(
        self,
        memory: &mut AddressSpace,
        set: &mut ModuleSet32,
    ) -> Result<ModuleLoad32, ModuleError32> {
        self.install_with_writes(memory, set, &[])
    }

    pub fn install_with_writes(
        mut self,
        memory: &mut AddressSpace,
        set: &mut ModuleSet32,
        writes: &[WordWrite32],
    ) -> Result<ModuleLoad32, ModuleError32> {
        if !Arc::ptr_eq(&set.identity, &self.set_identity)
            || memory.identity() != self.memory_identity
            || memory.transaction_version() != self.memory_version
            || set.revision != self.set_revision
        {
            return Err(ModuleError32::StalePlan);
        }
        let revision = set.revision.checked_add(1).ok_or(ModuleError32::Capacity)?;
        set.modules
            .try_reserve(self.additions.len())
            .map_err(|_| ModuleError32::Allocation)?;
        let mut new_ids = Vec::new();
        new_ids
            .try_reserve_exact(self.additions.len())
            .map_err(|_| ModuleError32::Allocation)?;
        new_ids.extend(self.additions.iter().map(LoadedModule32::id));
        let mut introduced_edges = Vec::new();
        for (id, dependencies) in &self.dependency_updates {
            let original = set.get(*id).ok_or(ModuleError32::StalePlan)?;
            for target in dependencies {
                if !original.dependencies.contains(target) {
                    introduced_edges
                        .try_reserve(1)
                        .map_err(|_| ModuleError32::Allocation)?;
                    introduced_edges.push((*id, *target));
                }
            }
        }
        self.pages.sort_unstable_by_key(|page| page.address.0);
        if !self.pages.is_empty() {
            let mut rows = Vec::new();
            rows.try_reserve_exact(self.pages.len())
                .map_err(|_| ModuleError32::Allocation)?;
            rows.extend(self.pages.iter().map(|page| PageInitialization {
                address: page.address,
                permissions: page.permissions,
                bytes: page.bytes.as_slice(),
            }));
            memory.install_pages_with_word_writes(&rows, writes)?;
        } else if !writes.is_empty() {
            memory.write_words32(writes)?;
        }
        for (id, dependencies) in self.dependency_updates {
            let module = set
                .modules
                .iter_mut()
                .find(|module| module.id == id)
                .unwrap();
            module.dependencies = dependencies;
        }
        set.modules.append(&mut self.additions);
        set.next_id = self.next_id;
        set.forwarder_limit = self.forwarder_limit;
        set.root = set.root.or(Some(self.root));
        set.revision = revision;
        Ok(ModuleLoad32 {
            root: self.root,
            new_ids,
            initialization_order: self.initialization_order,
            introduced_edges,
        })
    }
}

pub use unload::UnloadPlan32;
