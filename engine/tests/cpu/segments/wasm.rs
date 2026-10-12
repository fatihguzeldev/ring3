#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "context/wasm/roundtrip.rs"]
mod cpu_extended_context_wasm;

#[path = "repeat/wasm/overrides.rs"]
mod cpu_rep_segments_wasm;

#[path = "cache/wasm/loads.rs"]
mod cpu_segments_wasm;

#[path = "selectors/wasm/adjust_privilege.rs"]
mod cpu_arpl_wasm;

#[path = "selectors/wasm/verify.rs"]
mod cpu_segment_queries_wasm;
