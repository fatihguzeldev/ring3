use super::ResourceError32 as E;
use crate::{
    loader::{ModuleId32, ResourceIdentifier32},
    process::{EngineInstance, HostError},
};

pub(in crate::process::windows) struct ResourcePayload32 {
    pub module: ModuleId32,
    pub entry: u32,
    pub language: u16,
    pub bytes: Vec<u8>,
}

impl EngineInstance {
    pub(in crate::process::windows) fn resource_module_identity(
        &self,
        handle: u32,
    ) -> Result<ModuleId32, HostError> {
        self.resource_module(handle)
            .map(|module| module.id)
            .map_err(HostError::Resource)
    }

    pub(in crate::process::windows) fn user32_resource_bytes(
        &self,
        instance: u32,
        kind: u16,
        name_address: u32,
        wide: bool,
        max_bytes: usize,
    ) -> Result<Vec<u8>, HostError> {
        self.user32_resource_payload(instance, kind, name_address, wide, max_bytes)
            .map(|payload| payload.bytes)
    }

    pub(in crate::process::windows) fn user32_resource_payload(
        &self,
        instance: u32,
        kind: u16,
        name_address: u32,
        wide: bool,
        max_bytes: usize,
    ) -> Result<ResourcePayload32, HostError> {
        let name = self.user32_resource_identifier(name_address, wide)?;
        self.resource_data_payload(
            instance,
            &ResourceIdentifier32::Id(u32::from(kind)),
            &name,
            0,
            max_bytes,
        )
    }

    pub(in crate::process::windows) fn user32_resource_identifier(
        &self,
        address: u32,
        wide: bool,
    ) -> Result<ResourceIdentifier32, HostError> {
        super::memory::identifier(
            self.memory.as_ref().ok_or(HostError::Closed)?,
            address,
            wide,
            self.windows_ansi_code_page(),
        )
        .map_err(HostError::Resource)
    }

    pub(in crate::process::windows) fn resource_payload_for_module(
        &self,
        id: ModuleId32,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        language: u16,
        max_bytes: usize,
    ) -> Result<ResourcePayload32, HostError> {
        let module = self.resource_module_id(id).map_err(HostError::Resource)?;
        self.resource_module_payload(module, kind, name, language, max_bytes)
    }

    pub(in crate::process) fn resource_data_bytes(
        &self,
        handle: u32,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        language: u16,
        max_bytes: usize,
    ) -> Result<Vec<u8>, HostError> {
        self.resource_data_payload(handle, kind, name, language, max_bytes)
            .map(|payload| payload.bytes)
    }

    pub(in crate::process::windows) fn resource_data_payload(
        &self,
        handle: u32,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        language: u16,
        max_bytes: usize,
    ) -> Result<ResourcePayload32, HostError> {
        let module = self.resource_module(handle).map_err(HostError::Resource)?;
        let preferred = if language == 0 {
            self.current_thread_ui_language()
        } else {
            language
        };
        self.resource_module_payload(module, kind, name, preferred, max_bytes)
    }

    fn resource_module_payload(
        &self,
        module: super::context::ModuleResource32,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        language: u16,
        max_bytes: usize,
    ) -> Result<ResourcePayload32, HostError> {
        let result = (|| {
            let preferred = language;
            let mut reader = self.resource_reader(module)?;
            let (location, language) = reader.find_languages_with_language(
                kind,
                name,
                &[preferred, preferred & 0x3ff, 0, 0x409, 9],
                true,
            )?;
            if location.size as usize > max_bytes {
                return Err(E::Capacity);
            }
            let address = reader.address(location.data_rva, location.size)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(location.size as usize)
                .map_err(|_| E::Allocation)?;
            bytes.resize(location.size as usize, 0);
            self.memory
                .as_ref()
                .ok_or(E::NotConfigured)?
                .read(address, &mut bytes)?;
            Ok::<_, E>(ResourcePayload32 {
                module: module.id,
                entry: location.entry_rva,
                language,
                bytes,
            })
        })();
        result.map_err(HostError::Resource)
    }
}
