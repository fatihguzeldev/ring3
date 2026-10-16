use super::super::{ProcessStartup32, allocator::StartupArena32};
use super::{LoaderEntry32, entry_bytes, list_words, module_path};
use crate::{
    loader::{LoadedModule32, ModuleId32},
    memory::{GuestAddress, WordWrite32},
    process::{EngineInstance, HostError},
    windows::process::ProcessError32,
};

pub(in crate::process) struct PreparedProcessLoader32 {
    arena: StartupArena32,
    entries: Vec<LoaderEntry32>,
    initialized: Vec<ModuleId32>,
    words: Vec<WordWrite32>,
}
impl PreparedProcessLoader32 {
    pub(in crate::process) fn words(&self) -> &[WordWrite32] {
        &self.words
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_process_loader_change(
        &self,
        additions: &[LoadedModule32],
        initialization_order: &[ModuleId32],
        removed: &[ModuleId32],
    ) -> Result<Option<PreparedProcessLoader32>, HostError> {
        let Some(startup) = self.process_runtime.startup.as_ref() else {
            return Ok(None);
        };
        if additions.is_empty() && removed.is_empty() {
            return Ok(None);
        }
        let system = self
            .files
            .as_ref()
            .map(|files| files.virtual_system_directory())
            .map_or_else(
                || "C:\\Windows\\System32".encode_utf16().collect::<Vec<_>>(),
                <[u16]>::to_vec,
            );
        startup
            .prepare_loader_change(additions, initialization_order, removed, &system)
            .map(Some)
            .map_err(HostError::WindowsProcess)
    }
    pub(in crate::process) fn commit_process_loader_change(
        &mut self,
        prepared: Option<PreparedProcessLoader32>,
    ) {
        if let Some(prepared) = prepared {
            self.process_runtime
                .startup
                .as_mut()
                .expect("prepared process loader arena remains live")
                .commit_loader_change(prepared);
        }
    }
}
impl ProcessStartup32 {
    pub(in crate::process::windows::process::startup) fn prepare_loader_change(
        &self,
        additions: &[LoadedModule32],
        initialization_order: &[ModuleId32],
        removed: &[ModuleId32],
        system: &[u16],
    ) -> Result<PreparedProcessLoader32, ProcessError32> {
        let mut arena = self.prepare_arena()?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(self.loader_entries.len() + additions.len())
            .map_err(|_| ProcessError32::Allocation)?;
        for entry in &self.loader_entries {
            if removed.contains(&entry.id) {
                arena.release(entry.address, entry.length)?;
            } else {
                entries.push(entry.clone());
            }
        }
        let mut initialized = Vec::new();
        initialized
            .try_reserve_exact(self.loader_initialization.len() + additions.len())
            .map_err(|_| ProcessError32::Allocation)?;
        initialized.extend(
            self.loader_initialization
                .iter()
                .copied()
                .filter(|id| !removed.contains(id)),
        );
        let mut words = Vec::new();
        for module in additions {
            if removed.contains(&module.id()) || entries.iter().any(|entry| entry.id == module.id())
            {
                return Err(ProcessError32::InvalidParameter);
            }
            let full = module_path(module, &self.image_path, system);
            let mut encoded = entry_bytes(module, 0, &full)?;
            let address = arena.allocate(encoded.len(), 4)?;
            for offset in [40, 48] {
                let relative = u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap());
                encoded[offset..offset + 4].copy_from_slice(&(address + relative).to_le_bytes());
            }
            words
                .try_reserve_exact(encoded.len() / 4)
                .map_err(|_| ProcessError32::Allocation)?;
            for (index, value) in encoded.chunks_exact(4).enumerate().skip(6) {
                words.push(WordWrite32 {
                    address: GuestAddress(address + index as u32 * 4),
                    value: u32::from_le_bytes(value.try_into().unwrap()),
                });
            }
            if !module.is_dll() || module.is_uninitialized() {
                for offset in [16, 20] {
                    words.push(WordWrite32 {
                        address: GuestAddress(address + offset),
                        value: 0,
                    });
                }
            }
            entries.push(LoaderEntry32 {
                id: module.id(),
                address,
                length: encoded.len(),
                dll: module.is_dll(),
                uninitialized: module.is_uninitialized(),
                full_name: full.into(),
            });
        }
        for &id in initialization_order {
            if entries.iter().any(|entry| entry.id == id && entry.dll) && !initialized.contains(&id)
            {
                initialized.push(id);
            }
        }
        if entries
            .iter()
            .any(|entry| entry.dll && !entry.uninitialized && !initialized.contains(&entry.id))
        {
            return Err(ProcessError32::InvalidParameter);
        }
        let links = list_words(self.peb, &entries, &initialized)?;
        words
            .try_reserve_exact(links.len())
            .map_err(|_| ProcessError32::Allocation)?;
        words.extend(links);
        words.sort_unstable_by_key(|word| word.address.0);
        Ok(PreparedProcessLoader32 {
            arena,
            entries,
            initialized,
            words,
        })
    }
    pub(in crate::process::windows::process::startup) fn commit_loader_change(
        &mut self,
        prepared: PreparedProcessLoader32,
    ) {
        self.commit_arena(prepared.arena);
        self.loader_entries = prepared.entries;
        self.loader_initialization = prepared.initialized;
    }
}
