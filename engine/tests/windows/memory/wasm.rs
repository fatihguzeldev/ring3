#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "wasm/executable.rs"]
mod windows_allocation_protection_wasm;

#[path = "wasm/flush.rs"]
mod windows_flush_instruction_cache_wasm;

#[path = "wasm/allocation.rs"]
mod windows_virtual_alloc_wasm;

#[path = "release/wasm/linked.rs"]
mod windows_virtual_free_linked_wasm;

#[path = "release/wasm/manual.rs"]
mod windows_virtual_free_wasm;

#[path = "wasm/protection.rs"]
mod windows_virtual_protect_wasm;
