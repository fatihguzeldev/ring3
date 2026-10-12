#[path = "callbacks/native/reentry.rs"]
mod abi_callback;

#[path = "exit/native/core.rs"]
mod abi_exit;

#[path = "callbacks/native/resident.rs"]
mod abi_resident_callback;

#[path = "callbacks/native/result.rs"]
mod abi_resident_callback_result;

#[path = "native/state.rs"]
mod abi_state;

#[path = "helpers/exchange/native/byte.rs"]
mod byte_exchange_helper;

#[path = "helpers/store/native/byte.rs"]
mod byte_store_helper;

#[path = "native/call_frame.rs"]
mod call_frame_abi;

#[path = "exit/native/infrastructure.rs"]
mod cpu_exit_v2;

#[path = "exit/native/gate.rs"]
mod cpu_exit_v3;

#[path = "native/arena_defaults.rs"]
mod fixture_arena_defaults;

#[path = "helpers/native/dword.rs"]
mod memory_helper;

#[path = "helpers/native/narrow.rs"]
mod narrow_memory_helper;

#[path = "helpers/exchange/native/word.rs"]
mod word_exchange_helper;

#[path = "helpers/store/native/word.rs"]
mod word_store_helper;

#[path = "native/x87.rs"]
mod x87_arena;
