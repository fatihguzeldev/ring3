use crate::{process::wasm::mutate, windows::memory::MemoryLimits32};

pub(crate) fn configure_windows_memory(
    regions: u32,
    fragments: u32,
    heaps: u32,
    blocks: u32,
    moveable_handles: u32,
    heap_reserve_bytes: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_windows_memory(MemoryLimits32 {
            regions: regions as usize,
            fragments: fragments as usize,
            heaps: heaps as usize,
            blocks: blocks as usize,
            moveable_handles: moveable_handles as usize,
            heap_reserve_bytes,
        })
    })
}
