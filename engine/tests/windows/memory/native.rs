#[path = "native/flush.rs"]
mod windows_flush_instruction_cache;

#[path = "native/allocation.rs"]
mod windows_virtual_alloc;

#[path = "release/native/core.rs"]
mod windows_virtual_free;

#[path = "native/protection.rs"]
mod windows_virtual_protect;
