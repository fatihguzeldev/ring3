#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "cases/native/windows_call32.rs"]
mod windows_call32;

#[path = "cases/native/windows_callback_frame.rs"]
mod windows_callback_frame;

#[path = "cases/native/windows_callback_provider.rs"]
mod windows_callback_provider;

#[path = "cases/native/windows_current_process.rs"]
mod windows_current_process;

#[path = "cases/native/windows_module_handle.rs"]
mod windows_module_handle;

#[path = "cases/native/windows_provider.rs"]
mod windows_provider;
