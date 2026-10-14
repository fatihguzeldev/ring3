#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "integer/wasm/completion.rs"]
mod cpu_integer_completion_wasm;

#[path = "admission/wasm/boundaries.rs"]
mod cpu_profile_boundaries_wasm;

#[path = "environment/cpuid/wasm.rs"]
mod cpu_cpuid_wasm;

#[path = "environment/ordering/wasm.rs"]
mod cpu_cache_timestamp_wasm;

#[path = "environment/clock/wasm.rs"]
mod cpu_clock_wasm;

#[path = "environment/identity/wasm.rs"]
mod cpu_identity_wasm;
