#[path = "../../support/cpu/prefix_admission.rs"]
mod support_prefix_admission;

#[path = "entries/native/replacement.rs"]
mod process_entry_regions;

#[path = "lifecycle/disposal/inputs/native.rs"]
mod process_installed_resident_disposal_inputs;

#[path = "native/identity.rs"]
mod process_resident;

#[path = "admission/native/binary.rs"]
mod process_resident_binary;

#[path = "native/calls.rs"]
mod process_resident_call;

#[path = "admission/native/control.rs"]
mod process_resident_control;

#[path = "lifecycle/disposal/native.rs"]
mod process_resident_disposal;

#[path = "entries/native/resident.rs"]
mod process_resident_entries;

#[path = "admission/native/gates.rs"]
mod process_resident_gate;

#[path = "admission/native/indirect.rs"]
mod process_resident_indirect;

#[path = "lifecycle/native/installation.rs"]
mod process_resident_installation;

#[path = "admission/native/read.rs"]
mod process_resident_memory;

#[path = "lifecycle/native/retirement.rs"]
mod process_resident_retirement;

#[path = "admission/native/stack.rs"]
mod process_resident_stack;

#[path = "admission/native/store.rs"]
mod process_resident_store;

#[path = "admission/native/unary.rs"]
mod process_resident_unary;
