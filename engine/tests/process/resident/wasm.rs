#[path = "../../support/loader/pe32.rs"]
#[allow(dead_code)]
mod support_pe32;

#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "admission/wasm/binary.rs"]
mod process_resident_binary_wasm;

#[path = "wasm/calls.rs"]
mod process_resident_call_wasm;

#[path = "admission/wasm/control.rs"]
mod process_resident_control_wasm;

#[path = "entries/wasm/resident.rs"]
mod process_resident_entries_wasm;

#[path = "admission/wasm/gates.rs"]
mod process_resident_gate_wasm;

#[path = "admission/wasm/indirect.rs"]
mod process_resident_indirect_wasm;

#[path = "lifecycle/wasm/installation.rs"]
mod process_resident_installation_wasm;

#[path = "admission/wasm/read.rs"]
mod process_resident_memory_wasm;

#[path = "lifecycle/wasm/retirement.rs"]
mod process_resident_retirement_wasm;

#[path = "admission/wasm/stack.rs"]
mod process_resident_stack_wasm;

#[path = "admission/wasm/store.rs"]
mod process_resident_store_wasm;

#[path = "admission/wasm/unary.rs"]
mod process_resident_unary_wasm;

#[path = "wasm/identity.rs"]
mod process_resident_wasm;

#[path = "lifecycle/disposal/wasm.rs"]
mod process_unacknowledged_disposal_wasm;
