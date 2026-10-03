use super::{EngineInstance, HostError};
use crate::loader::{ImageMetadata32, LoadedPe32, load_pe32};

impl EngineInstance {
    pub fn load_pe32(&mut self, bytes: &[u8]) -> Result<ImageMetadata32, HostError> {
        let pages = self.image_capacity()?;
        let image = load_pe32(bytes, pages).map_err(HostError::Loader)?;
        Ok(self.publish_image(image))
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn load_pe32_transfer(&mut self, length: u32) -> Result<ImageMetadata32, HostError> {
        use crate::abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE};

        let pages = self.image_capacity()?;
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let bytes = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
        let image = load_pe32(bytes, pages).map_err(HostError::Loader)?;
        Ok(self.publish_image(image))
    }

    fn image_capacity(&self) -> Result<u32, HostError> {
        let memory = self.memory()?;
        if memory.mapped_pages() != 0
            || self.image.is_some()
            || self.generation != 0
            || self.artifact.is_some()
            || self.resident.is_some()
            || self.resident_installations.iter().any(Option::is_some)
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(memory.capacity_pages())
    }

    fn publish_image(&mut self, loaded: LoadedPe32) -> ImageMetadata32 {
        let (memory, image) = loaded.into_parts();
        self.memory = Some(memory);
        self.image = Some(image);
        image
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
            instance.map(0x1000, 1, 3),
            Err(HostError::Memory(MemoryError::VersionExhausted))
        );
        assert_eq!(instance.memory().unwrap().mapped_pages(), 0);
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
