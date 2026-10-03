mod pe32;

use crate::memory::{AddressSpace, MemoryError};

pub use pe32::load_pe32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageMetadata32 {
    pub image_base: u32,
    pub image_size: u32,
    pub entry_point: u32,
    pub mapped_pages: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Malformed,
    Unsupported,
    Capacity,
    Memory(MemoryError),
}

pub struct LoadedPe32 {
    memory: AddressSpace,
    metadata: ImageMetadata32,
}

impl LoadedPe32 {
    pub fn into_parts(self) -> (AddressSpace, ImageMetadata32) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedPe32 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedPe32")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}
