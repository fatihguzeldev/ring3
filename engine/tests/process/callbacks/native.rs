#[path = "replacement/native/code.rs"]
mod callback_code_install;

#[path = "resident/calls/active/native.rs"]
mod process_active_resident_callback_call;

#[path = "replacement/native/admission.rs"]
mod process_callbacks;

#[path = "resident/native/admission.rs"]
mod process_resident_callback;

#[path = "resident/calls/native.rs"]
mod process_resident_callback_call;

#[path = "resident/native/execution.rs"]
mod process_resident_callback_execution;

#[path = "resident/native/finish.rs"]
mod process_resident_callback_finish;

#[path = "resident/native/installation.rs"]
mod process_resident_callback_installation;

#[path = "resident/native/selection.rs"]
mod process_resident_callback_selection;
