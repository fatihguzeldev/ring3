use std::collections::BTreeMap;

use super::{
    Access, DispatchError, GuestMemory, MemoryError, Modules, Resource, Resources, Selection,
    guest, thread,
};

const FIRST_HANDLE: u32 = 0x7900_0004;
const LAST_HANDLE: u32 = 0x79ff_fffc;
const MAX_LOADED: usize = 1024;
const MAX_RESOURCE_SIZE: u32 = 16 * 1024 * 1024;

pub(super) struct Loaded {
    entries: BTreeMap<u32, Resource>,
    next: u32,
}

impl Default for Loaded {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl Resources {
    pub(super) fn load_resource(
        &mut self,
        args: &[u32],
        modules: &Modules,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let resource = match self.find_info(args[0], args[1], modules)? {
            Selection::Found(resource) => resource,
            Selection::Missing(error) => return failure(memory, error),
        };
        if let Some((&handle, _)) = self
            .loaded
            .entries
            .iter()
            .find(|(_, loaded)| loaded.info == resource.info && loaded.base == resource.base)
        {
            return Ok(handle);
        }
        if self.loaded.entries.len() == MAX_LOADED || self.loaded.next > LAST_HANDLE {
            return failure(memory, 8);
        }
        let address = payload_address(resource)?;
        if resource.size == 0 || resource.size > MAX_RESOURCE_SIZE {
            return Err(DispatchError::Unsupported);
        }
        guest::check(memory, address, resource.size as usize, Access::Read)?;
        let handle = self.loaded.next;
        self.loaded.entries.insert(handle, resource);
        self.loaded.next += 4;
        Ok(handle)
    }

    pub(super) fn lock_resource(
        &self,
        handle: u32,
        modules: &Modules,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let Some(&resource) = self.loaded.entries.get(&handle) else {
            return failure(memory, 87);
        };
        if !modules.contains(resource.base) {
            return failure(memory, 87);
        }
        let address = payload_address(resource)?;
        guest::check(memory, address, resource.size as usize, Access::Read)?;
        Ok(address)
    }

    pub(super) fn sizeof_resource(
        &self,
        args: &[u32],
        modules: &Modules,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match self.find_info(args[0], args[1], modules)? {
            Selection::Found(resource) => Ok(resource.size),
            Selection::Missing(error) => failure(memory, error),
        }
    }
}

fn payload_address(resource: Resource) -> Result<u32, MemoryError> {
    resource
        .base
        .checked_add(resource.data)
        .ok_or(MemoryError::AddressOverflow)
}

fn failure(memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(0)
}
