#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "exceptions/wasm/clear.rs"]
mod cpu_x87_clear_wasm;

#[path = "control/wasm/register.rs"]
mod cpu_x87_control_wasm;

#[path = "numeric/wasm/extended.rs"]
mod cpu_x87_extended_wasm;

#[path = "control/wasm/memory.rs"]
mod cpu_x87_memory_control_wasm;

#[path = "status/wasm/memory.rs"]
mod cpu_x87_memory_status_wasm;

#[path = "segment_access/wasm/cached.rs"]
mod cpu_x87_segment_access;

#[path = "control/compatibility/wasm.rs"]
mod cpu_x87_compatibility;

#[path = "protocol/wasm/helper.rs"]
mod cpu_x87_helper_protocol;
