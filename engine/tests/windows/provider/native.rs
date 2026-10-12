#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../calling/native/calls.rs"]
mod windows_call32;

#[path = "../calling/native/callbacks.rs"]
mod windows_callback_frame;

#[path = "callbacks/native/provider.rs"]
mod windows_callback_provider;

#[path = "native/current_process.rs"]
mod windows_current_process;

#[path = "native/module_handle.rs"]
mod windows_module_handle;

#[path = "native/core.rs"]
mod windows_provider;
