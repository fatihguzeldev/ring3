use crate::{
    loader::{
        ImageMetadata32, LinkedImageMetadata32, LinkedImageMetadata32V2, LinkedImageMetadata32V3,
        LinkedImageMetadata32V4, LinkedImageMetadata32V5, LinkedImageMetadata32V6,
        LinkedImageMetadata32V7, LinkedImageMetadata32V8, LinkedMetadata32, LoadedLinkedImage32,
        LoadedPe32, ProviderProfile, load_linked_profile, load_pe32, load_pe32_at,
    },
    memory::AddressSpace,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn load_pe32(&mut self, bytes: &[u8]) -> Result<ImageMetadata32, HostError> {
        let pages = self.image_capacity()?;
        let image = load_pe32(bytes, pages).map_err(HostError::Loader)?;
        Ok(self.publish_image(image))
    }

    pub fn load_pe32_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
    ) -> Result<ImageMetadata32, HostError> {
        let pages = self.image_capacity()?;
        let image = load_pe32_at(bytes, actual_base, pages).map_err(HostError::Loader)?;
        Ok(self.publish_image(image))
    }

    pub fn load_pe32_linked_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::LastErrorOnly,
        )
    }

    pub fn load_pe32_linked_v2_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V2, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::LastErrorAndExit,
        )
    }

    pub fn load_pe32_linked_v3_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V3, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::MainImageHandle,
        )
    }

    pub fn load_pe32_linked_v4_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V4, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::VirtualAllocation,
        )
    }

    pub fn load_pe32_linked_v5_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V5, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::VirtualMemory,
        )
    }

    pub fn load_pe32_linked_v6_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V6, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::VirtualProtection,
        )
    }

    pub fn load_pe32_linked_v7_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V7, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::InstructionCache,
        )
    }

    pub fn load_pe32_linked_v8_at(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V8, HostError> {
        self.load_linked_image(
            bytes,
            actual_base,
            gate_base,
            ProviderProfile::CurrentProcess,
        )
    }

    fn load_linked_image<M: From<LinkedMetadata32>>(
        &mut self,
        bytes: &[u8],
        actual_base: u32,
        gate_base: u32,
        profile: ProviderProfile,
    ) -> Result<M, HostError> {
        let pages = self.image_capacity()?;
        let loaded = load_linked_profile(bytes, actual_base, gate_base, pages, profile)
            .map_err(HostError::Loader)?;
        Ok(self.publish_linked_image(loaded))
    }

    pub(in crate::process) fn publish_linked_image<M: From<LinkedMetadata32>>(
        &mut self,
        loaded: LoadedLinkedImage32,
    ) -> M {
        let (memory, linked) = loaded.into_parts();
        self.publish_image_parts(memory, linked.image);
        linked.into()
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::process) fn load_pe32_linked_transfer_at(
        &mut self,
        length: u32,
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32, HostError> {
        self.load_linked_transfer(
            length,
            actual_base,
            gate_base,
            ProviderProfile::LastErrorOnly,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::process) fn load_pe32_linked_v2_transfer_at(
        &mut self,
        length: u32,
        actual_base: u32,
        gate_base: u32,
    ) -> Result<LinkedImageMetadata32V2, HostError> {
        self.load_linked_transfer(
            length,
            actual_base,
            gate_base,
            ProviderProfile::LastErrorAndExit,
        )
    }

    #[cfg(target_arch = "wasm32")]
    fn load_linked_transfer<M: From<LinkedMetadata32>>(
        &mut self,
        length: u32,
        actual_base: u32,
        gate_base: u32,
        profile: ProviderProfile,
    ) -> Result<M, HostError> {
        use crate::abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE};
        let pages = self.image_capacity()?;
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let bytes = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
        let loaded = load_linked_profile(bytes, actual_base, gate_base, pages, profile)
            .map_err(HostError::Loader)?;
        Ok(self.publish_linked_image(loaded))
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::process) fn load_pe32_transfer(
        &mut self,
        length: u32,
    ) -> Result<ImageMetadata32, HostError> {
        self.load_image_transfer(length, None)
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::process) fn load_pe32_transfer_at(
        &mut self,
        length: u32,
        actual_base: u32,
    ) -> Result<ImageMetadata32, HostError> {
        self.load_image_transfer(length, Some(actual_base))
    }

    #[cfg(target_arch = "wasm32")]
    fn load_image_transfer(
        &mut self,
        length: u32,
        actual_base: Option<u32>,
    ) -> Result<ImageMetadata32, HostError> {
        use crate::abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE};

        let pages = self.image_capacity()?;
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let bytes = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
        let image = match actual_base {
            None => load_pe32(bytes, pages),
            Some(base) => load_pe32_at(bytes, base, pages),
        }
        .map_err(HostError::Loader)?;
        Ok(self.publish_image(image))
    }

    pub(in crate::process) fn image_capacity(&self) -> Result<u32, HostError> {
        self.initial_image_capacity(false)
    }
    pub(in crate::process) fn module_process_capacity(&self) -> Result<u32, HostError> {
        self.initial_image_capacity(true)
    }
    fn initial_image_capacity(&self, retain_empty_registry: bool) -> Result<u32, HostError> {
        let memory = self.memory()?;
        if memory.mapped_pages() != 0
            || !self.windows_memory.is_empty()
            || self.image.is_some()
            || self.generation != 0
            || self.artifact.is_some()
            || self
                .resident
                .as_ref()
                .is_some_and(|registry| !retain_empty_registry || registry.usage().units != 0)
            || self.resident_installations.iter().any(Option::is_some)
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(memory.capacity_pages())
    }

    pub(in crate::process) fn publish_image(&mut self, loaded: LoadedPe32) -> ImageMetadata32 {
        let (memory, image) = loaded.into_parts();
        self.publish_image_parts(memory, image);
        image
    }

    fn publish_image_parts(&mut self, memory: AddressSpace, image: ImageMetadata32) {
        self.memory = Some(memory);
        self.image = Some(image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{loader::LoadError, memory::MemoryError};

    #[test]
    fn failed_pristine_load_keeps_identity_and_exhausted_memory_version() {
        let mut instance = EngineInstance::new(4, 7).unwrap();
        let identity = instance.memory().unwrap().identity();
        let arena = instance.arena().to_vec();
        instance
            .memory
            .as_mut()
            .unwrap()
            .exhaust_versions_for_test();
        assert_eq!(
            instance.load_pe32(b"bad"),
            Err(HostError::Loader(LoadError::Malformed))
        );
        assert_eq!(instance.memory().unwrap().identity(), identity);
        assert_eq!(instance.arena(), arena);
        assert!(instance.image.is_none());
        assert_eq!(
            instance.load_pe32_linked_at(b"bad", 0x0040_0000, 0x7000_0000),
            Err(HostError::Loader(LoadError::Malformed))
        );
        assert_eq!(instance.memory().unwrap().identity(), identity);
        assert_eq!(instance.arena(), arena);
        assert!(instance.image.is_none());
        assert_eq!(
            instance.map(0x1000, 1, 3),
            Err(HostError::Memory(MemoryError::VersionExhausted))
        );
        assert_eq!(instance.memory().unwrap().mapped_pages(), 0);
    }

    #[test]
    fn empty_configured_execution_survives_in_place_module_start_but_cannot_follow_flat_memory_replacement()
     {
        let mut instance = EngineInstance::new(8, 7).unwrap();
        let identity = instance.memory().unwrap().identity();
        instance.configure_execution(Default::default()).unwrap();
        assert_eq!(instance.module_process_capacity(), Ok(8));
        assert_eq!(instance.image_capacity(), Err(HostError::InvalidRequest));
        assert_eq!(instance.memory().unwrap().identity(), identity);
        assert_eq!(instance.execution_config().unwrap().unwrap().units, 256);
    }

    #[test]
    fn retained_generation_blocks_loading_even_with_empty_ram() {
        let mut instance = EngineInstance::new(1, 7).unwrap();
        instance.generation = 1;
        let identity = instance.memory().unwrap().identity();
        let arena = instance.arena().to_vec();
        assert_eq!(instance.load_pe32(b"bad"), Err(HostError::InvalidRequest));
        assert_eq!(instance.memory().unwrap().identity(), identity);
        assert_eq!(instance.arena(), arena);
        assert_eq!(instance.generation, 1);
    }
}

#[cfg(test)]
#[path = "image/tests/linked.rs"]
mod linked_v2_tests;
