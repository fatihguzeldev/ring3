use super::*;
use crate::{
    memory::AddressSpace,
    process::{EngineInstance, windows::process::ProcessStartup32},
    windows::process::ProcessError32,
};
impl EngineInstance {
    pub(in crate::process) fn module_source_file_exists(&self, path: &[u16]) -> bool {
        self.files.as_ref().is_some_and(|files| {
            files
                .vfs
                .path_info(path)
                .is_ok_and(|info| info.attributes & 0x10 == 0)
        })
    }
    pub(in crate::process) fn capture_module_file_paths(
        &self,
    ) -> crate::files::path::FilePathScope32 {
        self.files.as_ref().map_or_else(
            || {
                crate::files::path::FilePathScope32::from_directory(
                    self.process_launch_directory()
                        .unwrap_or("C:\\game")
                        .encode_utf16()
                        .collect(),
                )
            },
            |files| files.vfs.capture_path_scope(),
        )
    }
    pub(in crate::process) fn module_file_search_directories(
        &self,
        include_current: bool,
    ) -> Vec<Vec<u16>> {
        if let Some(files) = &self.files {
            let environment = files.file_environment();
            let mut system16 = environment.windows_directory.clone();
            if system16.last() != Some(&92) {
                system16.push(92);
            }
            system16.extend("System".encode_utf16());
            let mut paths = vec![
                environment.system_directory.clone(),
                system16,
                environment.windows_directory.clone(),
            ];
            if include_current {
                paths.push(files.vfs.current_directory().to_vec());
            }
            paths.extend(environment.search_path.iter().cloned());
            paths
        } else {
            let mut paths = vec![
                "C:\\Windows\\System32".encode_utf16().collect(),
                "C:\\Windows\\System".encode_utf16().collect(),
                "C:\\Windows".encode_utf16().collect(),
            ];
            if include_current {
                paths.push(
                    self.process_launch_directory()
                        .unwrap_or("C:\\game")
                        .encode_utf16()
                        .collect(),
                );
            }
            paths
        }
    }

    pub(in crate::process) fn apply_launch_file_directory(
        &mut self,
        value: &str,
    ) -> Result<(), crate::process::HostError> {
        if let Some(files) = self.files.as_mut() {
            let directory = value.encode_utf16().collect::<Vec<_>>();
            let prepared = files
                .vfs
                .prepare_current_directory(&directory)
                .map_err(crate::process::HostError::File)?;
            prepared
                .validate(&files.vfs)
                .map_err(crate::process::HostError::File)?;
            files.vfs.commit_namespace(prepared);
        }
        Ok(())
    }

    pub(in crate::process) fn configured_file_current_directory(&self) -> Option<&[u16]> {
        self.files
            .as_ref()
            .map(|files| files.vfs.current_directory())
    }
}
impl FileRuntime32 {
    pub(super) fn set_process_current_directory(
        &mut self,
        memory: &mut AddressSpace,
        startup: &mut ProcessStartup32,
        args: &[u32],
        wide: bool,
    ) -> Result<FileCallOutcome32, FileError32> {
        let path = ordinary::memory::path(memory, args[0], wide, self.ordinary.ansi)?;
        self.set_process_current_directory_units(memory, startup, &path)?;
        Ok(FileCallOutcome32::success(1))
    }
    fn set_process_current_directory_units(
        &mut self,
        memory: &mut AddressSpace,
        startup: &mut ProcessStartup32,
        path: &[u16],
    ) -> Result<(), FileError32> {
        let absolute = self.vfs.full_path(path)?;
        let prepared = self.vfs.prepare_current_directory(&absolute)?;
        prepared.validate(&self.vfs)?;
        startup
            .replace_current_directory(memory, &absolute)
            .map_err(|error| match error {
                ProcessError32::Capacity => FileError32::Capacity,
                ProcessError32::Allocation => FileError32::Allocation,
                ProcessError32::NoAccess => FileError32::NoAccess,
                _ => FileError32::InvalidParameter,
            })?;
        self.vfs.commit_namespace(prepared);
        Ok(())
    }
}

impl EngineInstance {
    pub(in crate::process) fn set_provider_current_directory(
        &mut self,
        path: &[u16],
    ) -> Result<(), crate::process::HostError> {
        use crate::process::HostError;
        let files = self.files.as_mut().ok_or(HostError::InvalidRequest)?;
        let absolute = files.vfs.full_path(path).map_err(HostError::File)?;
        files
            .vfs
            .check_path_access(&absolute, 0x20, self.windows_security.token())
            .map_err(HostError::File)?;
        if let Some(startup) = self.process_runtime.startup.as_mut() {
            files
                .set_process_current_directory_units(
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    startup,
                    &absolute,
                )
                .map_err(HostError::File)
        } else {
            let prepared = files
                .vfs
                .prepare_current_directory(&absolute)
                .map_err(HostError::File)?;
            prepared.validate(&files.vfs).map_err(HostError::File)?;
            files.vfs.commit_namespace(prepared);
            Ok(())
        }
    }
}
