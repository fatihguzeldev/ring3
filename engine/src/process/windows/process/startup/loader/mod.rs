mod entries;
mod links;
mod names;
mod plan;

use super::bytes::StartupBytes32;
use crate::{
    loader::{LoadedModule32, ModuleId32},
    process::{EngineInstance, HostError},
    windows::process::ProcessError32,
};
pub(in crate::process::windows::process::startup) use entries::LoaderEntry32;
use entries::{entry_bytes, module_path};
use links::list_words;

impl EngineInstance {
    pub(in crate::process::windows::process::startup) fn stage_process_loader(
        &self,
        bytes: &mut StartupBytes32,
        additions: &[LoadedModule32],
        order: &[ModuleId32],
        image_path: &str,
    ) -> Result<(Vec<LoaderEntry32>, Vec<ModuleId32>), HostError> {
        let system = self
            .files
            .as_ref()
            .map(|files| files.virtual_system_directory())
            .map_or_else(
                || "C:\\Windows\\System32".encode_utf16().collect::<Vec<_>>(),
                <[u16]>::to_vec,
            );
        stage_loader(bytes, additions, order, image_path, &system)
            .map_err(HostError::WindowsProcess)
    }
}
fn stage_loader(
    bytes: &mut StartupBytes32,
    additions: &[LoadedModule32],
    order: &[ModuleId32],
    image_path: &str,
    system: &[u16],
) -> Result<(Vec<LoaderEntry32>, Vec<ModuleId32>), ProcessError32> {
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(additions.len())
        .map_err(|_| ProcessError32::Allocation)?;
    for module in additions {
        let full = module_path(module, image_path, system);
        let address = bytes.base + (bytes.used.div_ceil(4) * 4) as u32;
        let encoded = entry_bytes(module, address, &full)?;
        bytes.put(&encoded, 4)?;
        entries.push(LoaderEntry32 {
            id: module.id(),
            address,
            length: encoded.len(),
            dll: module.is_dll(),
            full_name: full.into(),
        });
    }
    let mut initialized = Vec::new();
    initialized
        .try_reserve_exact(entries.len())
        .map_err(|_| ProcessError32::Allocation)?;
    for &id in order {
        if entries.iter().any(|entry| entry.id == id && entry.dll) {
            initialized.push(id);
        }
    }
    for word in list_words(bytes.base, &entries, &initialized)? {
        bytes.word((word.address.0 - bytes.base) as usize, word.value);
    }
    Ok((entries, initialized))
}

#[cfg(test)]
pub(in crate::process::windows::process) mod tests;
