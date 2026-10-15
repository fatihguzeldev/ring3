use super::{image::*, load::load_linked_profile};
use crate::loader::{LoadError, ProviderProfile};

pub fn load_pe32_linked_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::LastErrorOnly,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [value.gates[0], value.gates[1]],
        }
    }
}

pub fn load_pe32_linked_v2_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V2, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::LastErrorAndExit,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V2 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V2 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [value.gates[0], value.gates[1], value.gates[2]],
        }
    }
}

pub fn load_pe32_linked_v3_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V3, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::MainImageHandle,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V3 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V3 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [
                value.gates[0],
                value.gates[1],
                value.gates[2],
                value.gates[3],
            ],
        }
    }
}

pub fn load_pe32_linked_v4_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V4, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::VirtualAllocation,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V4 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V4 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [
                value.gates[0],
                value.gates[1],
                value.gates[2],
                value.gates[3],
                value.gates[4],
            ],
        }
    }
}

pub fn load_pe32_linked_v5_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V5, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::VirtualMemory,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V5 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V5 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [
                value.gates[0],
                value.gates[1],
                value.gates[2],
                value.gates[3],
                value.gates[4],
                value.gates[5],
            ],
        }
    }
}

pub fn load_pe32_linked_v6_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V6, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::VirtualProtection,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V6 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V6 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [
                value.gates[0],
                value.gates[1],
                value.gates[2],
                value.gates[3],
                value.gates[4],
                value.gates[5],
                value.gates[6],
            ],
        }
    }
}

pub fn load_pe32_linked_v7_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V7, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::InstructionCache,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V7 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V7 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [
                value.gates[0],
                value.gates[1],
                value.gates[2],
                value.gates[3],
                value.gates[4],
                value.gates[5],
                value.gates[6],
                value.gates[7],
            ],
        }
    }
}

pub fn load_pe32_linked_v8_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V8, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::CurrentProcess,
    )?;
    let (memory, metadata) = linked.into_parts();
    Ok(LoadedLinkedPe32V8 {
        memory,
        metadata: metadata.into(),
    })
}

impl From<LinkedMetadata32> for LinkedImageMetadata32V8 {
    fn from(value: LinkedMetadata32) -> Self {
        Self {
            image: value.image,
            gate_base: value.gate_base,
            gate_count: value.gate_count,
            gates: [
                value.gates[0],
                value.gates[1],
                value.gates[2],
                value.gates[3],
                value.gates[4],
                value.gates[5],
                value.gates[6],
                value.gates[7],
            ],
        }
    }
}
