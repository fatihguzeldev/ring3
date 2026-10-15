use crate::{cpu::dbt::GateSpec, loader::ImageMetadata32, memory::AddressSpace};

#[derive(Clone, Copy)]
pub(crate) struct LinkedMetadata32 {
    pub(crate) image: ImageMetadata32,
    pub(crate) gate_base: u32,
    pub(crate) gate_count: u32,
    pub(crate) gates: [GateSpec; 8],
}

pub(crate) struct LoadedLinkedImage32 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedMetadata32,
}

impl LoadedLinkedImage32 {
    pub(crate) fn into_parts(self) -> (AddressSpace, LinkedMetadata32) {
        (self.memory, self.metadata)
    }
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

pub struct LoadedLinkedPe32 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32,
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
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V2,
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
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V3,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V4 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 5],
}

pub struct LoadedLinkedPe32V4 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V4,
}

impl LoadedLinkedPe32V4 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V4) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V4 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V4")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V5 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 6],
}

pub struct LoadedLinkedPe32V5 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V5,
}

impl LoadedLinkedPe32V5 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V5) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V5 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V5")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V6 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 7],
}

pub struct LoadedLinkedPe32V6 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V6,
}

impl LoadedLinkedPe32V6 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V6) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V6 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V6")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V7 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 8],
}

pub struct LoadedLinkedPe32V7 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V7,
}

impl LoadedLinkedPe32V7 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V7) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V7 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V7")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedImageMetadata32V8 {
    pub image: ImageMetadata32,
    pub gate_base: u32,
    pub gate_count: u32,
    pub gates: [GateSpec; 8],
}

pub struct LoadedLinkedPe32V8 {
    pub(super) memory: AddressSpace,
    pub(super) metadata: LinkedImageMetadata32V8,
}

impl LoadedLinkedPe32V8 {
    pub fn into_parts(self) -> (AddressSpace, LinkedImageMetadata32V8) {
        (self.memory, self.metadata)
    }
}

impl std::fmt::Debug for LoadedLinkedPe32V8 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedLinkedPe32V8")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}
