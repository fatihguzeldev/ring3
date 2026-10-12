#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/windows_allocation_protection_wasm.rs"]
mod windows_allocation_protection_wasm;

#[path = "cases/wasm/windows_flush_instruction_cache_wasm.rs"]
mod windows_flush_instruction_cache_wasm;

#[path = "cases/wasm/windows_virtual_alloc_wasm.rs"]
mod windows_virtual_alloc_wasm;

#[path = "cases/wasm/windows_virtual_free_linked_wasm.rs"]
mod windows_virtual_free_linked_wasm;

#[path = "cases/wasm/windows_virtual_free_wasm.rs"]
mod windows_virtual_free_wasm;

#[path = "cases/wasm/windows_virtual_protect_wasm.rs"]
mod windows_virtual_protect_wasm;
