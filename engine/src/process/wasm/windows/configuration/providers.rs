use crate::{
    memory::{GuestAddress, PageRange},
    process::{HostError, Ole32Config32, wasm::mutate},
};

pub(crate) fn configure_ole32(base: u32, pages: u32, objects: u32, threads: u32) -> u32 {
    mutate(|instance| {
        instance.configure_ole32(Ole32Config32 {
            objects: PageRange::new(GuestAddress(base), pages).map_err(HostError::Memory)?,
            max_objects: objects,
            max_threads: threads,
        })
    })
}
pub(crate) fn register_ole32_module(base: u32) -> u32 {
    mutate(|instance| instance.register_ole32_module(base))
}
pub(crate) fn register_winmm_module(base: u32) -> u32 {
    mutate(|instance| instance.register_winmm_module(base))
}

pub(crate) fn configure_resources(
    base: u32,
    pages: u32,
    max_handles: u32,
    max_callbacks: u32,
    max_enum_bytes: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_resources(crate::process::ResourceConfig32 {
            scratch: PageRange::new(GuestAddress(base), pages).map_err(HostError::Memory)?,
            max_handles,
            max_callbacks,
            max_enum_bytes,
        })
    })
}

pub(crate) fn register_version_module(base: u32) -> u32 {
    mutate(|instance| instance.register_version_module(base))
}
pub(crate) fn register_shlwapi_module(base: u32) -> u32 {
    mutate(|instance| instance.register_shlwapi_module(base))
}
