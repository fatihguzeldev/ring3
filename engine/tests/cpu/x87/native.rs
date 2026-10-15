#[path = "../../support/cpu/prefix/admission.rs"]
mod support_prefix_admission;

#[path = "../../support/cpu/fetch/native.rs"]
mod support_fetch;

#[path = "exceptions/native/clear.rs"]
mod cpu_x87_clear;

#[path = "control/native/register.rs"]
mod cpu_x87_control;

#[path = "protocol/native/helper.rs"]
mod cpu_x87_helper_protocol;

#[path = "control/native/memory.rs"]
mod cpu_x87_memory_control;

#[path = "status/native/memory.rs"]
mod cpu_x87_memory_status;

#[path = "numeric/native/boundaries.rs"]
mod x87_numeric_boundaries;

#[path = "control/compatibility/native.rs"]
mod cpu_x87_compatibility;

#[path = "stack/compatibility/native.rs"]
mod cpu_x87_stack_compatibility;
