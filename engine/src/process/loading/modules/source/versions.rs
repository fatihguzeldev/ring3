use crate::{
    files::{FileContentVersion32, FileError32},
    process::{EngineInstance, HostError},
};

pub(in crate::process::loading::modules) struct ModuleVfsSource32 {
    pub(super) identity: String,
    pub(super) version: FileContentVersion32,
}

impl EngineInstance {
    pub(super) fn refresh_module_file_sources(&mut self) -> Result<(), HostError> {
        let mut index = 0;
        while index < self.modules.vfs_sources.len() {
            let source = &self.modules.vfs_sources[index];
            if self.modules.loaded.find(&source.identity).is_some() {
                index += 1;
                continue;
            }
            let current =
                self.provider_path_version(&source.identity.encode_utf16().collect::<Vec<_>>());
            let unchanged = match current {
                Ok(version) => version == source.version,
                Err(HostError::File(FileError32::NotFound | FileError32::PathNotFound)) => false,
                Err(error) => return Err(error),
            };
            if unchanged {
                index += 1;
            } else {
                self.modules
                    .catalog
                    .retire_guest_snapshot(&source.identity)
                    .map_err(HostError::Module)?;
                self.modules.vfs_sources.remove(index);
            }
        }
        Ok(())
    }
}
