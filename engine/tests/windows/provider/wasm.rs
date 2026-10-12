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

#[path = "process/current/wasm.rs"]
mod windows_current_process_wasm;

#[path = "modules/handle/wasm.rs"]
mod windows_module_handle_wasm;
