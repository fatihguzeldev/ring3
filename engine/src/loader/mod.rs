mod imports;
mod pe32;
mod relocation;

use crate::cpu::dbt::GateSpec;
use crate::memory::{AddressSpace, MemoryError};

pub use pe32::{
    load_pe32, load_pe32_at, load_pe32_linked_at, load_pe32_linked_v2_at, load_pe32_linked_v3_at,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageMetadata32 {
    pub image_base: u32,
    pub image_size: u32,
    pub entry_point: u32,
    pub mapped_pages: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V2 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 3],
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

pub struct LoadedLinkedPe32 {
    memory: AddressSpace,
    metadata: LinkedImageMetadata32,
}

impl LoadedLinkedPe32 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

pub struct LoadedLinkedPe32V2 {
    memory: AddressSpace,
    metadata: LinkedImageMetadata32V2,
}

impl LoadedLinkedPe32V2 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V2) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V2")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V3 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 4],
}

pub struct LoadedLinkedPe32V3 {
    memory: AddressSpace,
    metadata: LinkedImageMetadata32V3,
}

impl LoadedLinkedPe32V3 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V3) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V3")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}
