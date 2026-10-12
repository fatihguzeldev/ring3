#[path = "native/snapshots.rs"]
mod code_memory;

#[path = "native/space.rs"]
mod memory;

#[path = "native/allocation.rs"]
mod memory_allocation;

#[path = "native/exchange.rs"]
mod memory_exchange32;

#[path = "../process/memory/scalar/native/narrow.rs"]
mod narrow_reads;

#[path = "../process/memory/store/native/word.rs"]
mod word_store;
