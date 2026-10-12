#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "cases/wasm/process_resident_binary_wasm.rs"]
mod process_resident_binary_wasm;

#[path = "cases/wasm/process_resident_call_wasm.rs"]
mod process_resident_call_wasm;

#[path = "cases/wasm/process_resident_control_wasm.rs"]
mod process_resident_control_wasm;

#[path = "cases/wasm/process_resident_entries_wasm.rs"]
mod process_resident_entries_wasm;

#[path = "cases/wasm/process_resident_gate_wasm.rs"]
mod process_resident_gate_wasm;

#[path = "cases/wasm/process_resident_indirect_wasm.rs"]
mod process_resident_indirect_wasm;

#[path = "cases/wasm/process_resident_installation_wasm.rs"]
mod process_resident_installation_wasm;

#[path = "cases/wasm/process_resident_memory_wasm.rs"]
mod process_resident_memory_wasm;

#[path = "cases/wasm/process_resident_retirement_wasm.rs"]
mod process_resident_retirement_wasm;

#[path = "cases/wasm/process_resident_stack_wasm.rs"]
mod process_resident_stack_wasm;

#[path = "cases/wasm/process_resident_store_wasm.rs"]
mod process_resident_store_wasm;

#[path = "cases/wasm/process_resident_unary_wasm.rs"]
mod process_resident_unary_wasm;

#[path = "cases/wasm/process_resident_wasm.rs"]
mod process_resident_wasm;

#[path = "cases/wasm/process_unacknowledged_disposal_wasm.rs"]
mod process_unacknowledged_disposal_wasm;
