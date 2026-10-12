#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/process_windows_provider_wasm.rs"]
mod process_windows_provider_wasm;

#[path = "cases/wasm/windows_callback_exit_wasm.rs"]
mod windows_callback_exit_wasm;

#[path = "cases/wasm/windows_callback_provider_wasm.rs"]
mod windows_callback_provider_wasm;

#[path = "cases/wasm/windows_current_process_wasm.rs"]
mod windows_current_process_wasm;

#[path = "cases/wasm/windows_module_handle_wasm.rs"]
mod windows_module_handle_wasm;
