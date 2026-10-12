#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "wasm/core.rs"]
mod process_windows_provider_wasm;

#[path = "callbacks/wasm/exit.rs"]
mod windows_callback_exit_wasm;

#[path = "callbacks/wasm/provider.rs"]
mod windows_callback_provider_wasm;

#[path = "wasm/current_process.rs"]
mod windows_current_process_wasm;

#[path = "wasm/module_handle.rs"]
mod windows_module_handle_wasm;
