#[path = "selectors/privilege/native.rs"]
mod cpu_arpl;
#[path = "selectors/native/verify.rs"]
mod cpu_segment_queries;
#[path = "cache/native/loads.rs"]
mod cpu_segments;

#[path = "selectors/attributes/native.rs"]
mod cpu_segment_rights_limit;
