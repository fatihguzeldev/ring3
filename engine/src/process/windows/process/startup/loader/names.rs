use crate::{
    process::{EngineInstance, HostError},
    windows::process::ProcessError32,
};

impl EngineInstance {
    pub(in crate::process) fn resource_module_file_name(
        &self,
        handle: u32,
    ) -> Result<Vec<u16>, HostError> {
        if handle & 3 == 0 {
            return self.virtual_module_file_name_units(handle);
        }
        let module = self
            .resource_only_modules()
            .find(|module| module.handle() == handle)
            .ok_or(HostError::WindowsProcess(ProcessError32::InvalidHandle))?;
        module.check(self.memory()?).map_err(HostError::Module)?;
        let startup = self
            .process_runtime
            .startup
            .as_ref()
            .ok_or(HostError::WindowsProcess(ProcessError32::InvalidParameter))?;
        Ok(super::entries::guest_library_path(
            module.identity(),
            &startup.image_path,
        ))
    }
    pub(in crate::process) fn virtual_module_file_name(
        &self,
        handle: u32,
    ) -> Result<String, HostError> {
        Ok(String::from_utf16_lossy(
            &self.virtual_module_file_name_units(handle)?,
        ))
    }
    pub(in crate::process) fn virtual_module_file_name_units(
        &self,
        handle: u32,
    ) -> Result<Vec<u16>, HostError> {
        let startup = self
            .process_runtime
            .startup
            .as_ref()
            .ok_or(HostError::WindowsProcess(ProcessError32::InvalidParameter))?;
        if handle == 0 || self.image.is_some_and(|image| image.image_base == handle) {
            return Ok(startup.image_path.encode_utf16().collect());
        }
        let module = self
            .loaded_windows_modules()
            .find(|module| module.base() == handle)
            .ok_or(HostError::WindowsProcess(ProcessError32::InvalidHandle))?;
        let entry = startup
            .loader_entries
            .iter()
            .find(|entry| entry.id == module.id())
            .ok_or(HostError::Infrastructure)?;
        Ok(entry.full_name.to_vec())
    }
}
