#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "replacement/wasm/code.rs"]
mod callback_code_wasm;

#[path = "replacement/wasm/admission.rs"]
mod callbacks_wasm;

#[path = "resident/calls/active/wasm.rs"]
mod process_active_resident_callback_call_wasm;

#[path = "resident/wasm/retry.rs"]
mod process_cold_callback_retry_wasm;

#[path = "resident/calls/wasm.rs"]
mod process_resident_callback_call_wasm;

#[path = "resident/wasm/execution.rs"]
mod process_resident_callback_execution_wasm;

#[path = "resident/wasm/finish.rs"]
mod process_resident_callback_finish_wasm;

#[path = "resident/wasm/installation.rs"]
mod process_resident_callback_installation_wasm;

#[path = "resident/wasm/selection.rs"]
mod process_resident_callback_selection_wasm;

#[path = "resident/wasm/admission.rs"]
mod process_resident_callback_wasm;
