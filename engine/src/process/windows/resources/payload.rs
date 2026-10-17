use super::ResourceError32 as E;
use crate::{
    loader::{ModuleId32, ResourceIdentifier32},
    process::{EngineInstance, HostError},
};

pub(in crate::process::windows) struct ResourcePayload32 {
    pub module: ModuleId32,
    pub entry: u32,
    pub bytes: Vec<u8>,
}

impl EngineInstance {
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
        let name = super::memory::identifier(
            self.memory.as_ref().ok_or(HostError::Closed)?,
            name_address,
            wide,
            self.windows_ansi_code_page(),
        )
        .map_err(HostError::Resource)?;
        self.resource_data_payload(
            instance,
            &ResourceIdentifier32::Id(u32::from(kind)),
            &name,
            0,
            max_bytes,
        )
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

    fn resource_data_payload(
        &self,
        handle: u32,
        kind: &ResourceIdentifier32,
        name: &ResourceIdentifier32,
        language: u16,
        max_bytes: usize,
    ) -> Result<ResourcePayload32, HostError> {
        let result = (|| {
            let module = self.resource_module(handle)?;
            let preferred = if language == 0 {
                self.current_thread_ui_language()
            } else {
                language
            };
            let mut reader = self.resource_reader(module)?;
            let location = reader.find_languages(
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
                bytes,
            })
        })();
        result.map_err(HostError::Resource)
    }
}
