use crate::memory::AddressSpace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageMetadata32 {
    pub image_base: u32,
    pub image_size: u32,
    pub entry_point: u32,
    pub mapped_pages: u32,
}

pub struct LoadedPe32 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: ImageMetadata32,
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
