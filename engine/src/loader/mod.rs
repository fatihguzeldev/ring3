mod image;
mod imports;
mod linking;
mod pe32;
mod relocation;

use crate::memory::MemoryError;

pub(crate) use crate::windows::ProviderProfile;
pub use image::{ImageMetadata32, LoadedPe32};
pub use linking::{
    LinkedImageMetadata32, LinkedImageMetadata32V2, LinkedImageMetadata32V3,
    LinkedImageMetadata32V4, LinkedImageMetadata32V5, LinkedImageMetadata32V6,
    LinkedImageMetadata32V7, LinkedImageMetadata32V8, LoadedLinkedPe32, LoadedLinkedPe32V2,
    LoadedLinkedPe32V3, LoadedLinkedPe32V4, LoadedLinkedPe32V5, LoadedLinkedPe32V6,
    LoadedLinkedPe32V7, LoadedLinkedPe32V8, load_pe32_linked_at, load_pe32_linked_v2_at,
    load_pe32_linked_v3_at, load_pe32_linked_v4_at, load_pe32_linked_v5_at, load_pe32_linked_v6_at,
    load_pe32_linked_v7_at, load_pe32_linked_v8_at,
};
pub(crate) use linking::{LinkedMetadata32, LoadedLinkedImage32, load_linked_profile};
pub use pe32::{load_pe32, load_pe32_at};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Malformed,
    Unsupported,
    Capacity,
    Memory(MemoryError),
}
