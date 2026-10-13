#[path = "../../support/wasm.rs"]
mod support_wasm;

#[path = "context/wasm/roundtrip.rs"]
mod cpu_extended_context_wasm;

#[path = "repeat/wasm/overrides.rs"]
mod cpu_rep_segments_wasm;

#[path = "cache/wasm/loads.rs"]
mod cpu_segments_wasm;

#[path = "cache/raw/wasm.rs"]
mod cpu_raw_descriptors_wasm;

#[path = "selectors/privilege/wasm.rs"]
mod cpu_arpl_wasm;

#[path = "selectors/verify/wasm.rs"]
mod cpu_segment_queries_wasm;

#[path = "selectors/attributes/wasm.rs"]
mod cpu_segment_rights_limit_wasm;

#[path = "base/wasm.rs"]
mod cpu_segment_base_wasm;

#[path = "code/fetch/wasm.rs"]
mod cpu_code_fetch_wasm;

#[path = "stack/base/wasm.rs"]
mod cpu_ss_base_wasm;

#[path = "code/base/wasm.rs"]
mod cpu_code_base_wasm;

#[path = "stack/b16/wasm.rs"]
mod cpu_ss_b16_wasm;

#[path = "code/defaults/wasm.rs"]
mod cpu_code_defaults_wasm;
