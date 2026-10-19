#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "wasm/entry.rs"]
mod process_dispatch_entry_wasm;

#[path = "wasm/returning.rs"]
mod process_returning_dispatcher_wasm;

#[path = "../execution/cache/wasm.rs"]
mod process_execution_cache_wasm;

#[path = "../execution/receipt/wasm.rs"]
mod process_execution_receipt_wasm;

#[path = "../execution/native_success/wasm.rs"]
mod process_execution_native_success_wasm;
