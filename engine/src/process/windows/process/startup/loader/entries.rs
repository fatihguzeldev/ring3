use super::super::bytes::wide_bytes;
use crate::{
    loader::{LoadedModule32, ModuleId32},
    windows::process::ProcessError32,
};

#[derive(Clone)]
pub(in crate::process::windows::process::startup) struct LoaderEntry32 {
    pub(in crate::process::windows::process::startup) id: ModuleId32,
    pub(in crate::process::windows::process::startup) address: u32,
    pub(in crate::process::windows::process::startup) length: usize,
    pub(in crate::process::windows::process::startup) dll: bool,
    pub(in crate::process::windows::process::startup) uninitialized: bool,
    pub(in crate::process::windows::process::startup) full_name: std::sync::Arc<[u16]>,
}

pub(super) fn entry_bytes(
    module: &LoadedModule32,
    address: u32,
    full_name: &[u16],
) -> Result<Vec<u8>, ProcessError32> {
    let full = wide_bytes(full_name)?;
    let base = wide_bytes(&module.base_name().encode_utf16().collect::<Vec<_>>())?;
    let length = (64 + full.len() + base.len()).div_ceil(4) * 4;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| ProcessError32::Allocation)?;
    bytes.resize(length, 0);
    bytes[24..28].copy_from_slice(&module.base().to_le_bytes());
    bytes[28..32].copy_from_slice(&module.entry().map_or(0, |entry| entry.0).to_le_bytes());
    bytes[32..36].copy_from_slice(&module.image_size().to_le_bytes());
    descriptor(&mut bytes[36..44], address + 64, full.len());
    descriptor(
        &mut bytes[44..52],
        address + 64 + full.len() as u32,
        base.len(),
    );
    bytes[64..64 + full.len()].copy_from_slice(&full);
    bytes[64 + full.len()..64 + full.len() + base.len()].copy_from_slice(&base);
    Ok(bytes)
}
fn descriptor(bytes: &mut [u8], address: u32, length: usize) {
    bytes[..2].copy_from_slice(&((length - 2) as u16).to_le_bytes());
    bytes[2..4].copy_from_slice(&(length as u16).to_le_bytes());
    bytes[4..].copy_from_slice(&address.to_le_bytes());
}

pub(super) fn module_path(
    module: &LoadedModule32,
    image_path: &str,
    system_directory: &[u16],
) -> Vec<u16> {
    if !module.is_dll() && !module.is_uninitialized() {
        return image_path.encode_utf16().collect();
    }
    if let Some(path) = module.file_path() {
        return path.encode_utf16().collect();
    }
    let mut path = if module.is_system() {
        system_directory.to_vec()
    } else {
        return guest_library_path(module.identity(), image_path);
    };
    if !path.is_empty() && !path.last().is_some_and(|unit| matches!(*unit, 47 | 92)) {
        path.push(92);
    }
    path.extend(module.identity().encode_utf16());
    path
}
pub(super) fn guest_library_path(identity: &str, image_path: &str) -> Vec<u16> {
    if identity.contains('\\') {
        return identity.encode_utf16().collect();
    }
    let mut path = image_path
        .rsplit_once(['\\', '/'])
        .map_or_else(Vec::new, |(directory, _)| {
            directory.encode_utf16().collect()
        });
    if !path.is_empty() && !path.last().is_some_and(|unit| matches!(*unit, 47 | 92)) {
        path.push(92);
    }
    path.extend(identity.encode_utf16());
    path
}
