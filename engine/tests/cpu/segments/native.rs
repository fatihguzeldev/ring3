#[path = "selectors/privilege/native.rs"]
mod cpu_arpl;
#[path = "selectors/verify/native.rs"]
mod cpu_segment_queries;
#[path = "cache/native/loads.rs"]
mod cpu_segments;

#[path = "selectors/attributes/native.rs"]
mod cpu_segment_rights_limit;

#[path = "base/native.rs"]
mod cpu_segment_base;

#[path = "code/fetch/native.rs"]
mod cpu_code_fetch;

#[path = "stack/base/native.rs"]
mod cpu_ss_base;

#[path = "stack/b16/native.rs"]
mod cpu_ss_b16;
